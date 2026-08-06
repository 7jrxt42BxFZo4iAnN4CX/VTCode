use crate::core::agent::events::{EventSink, SharedLifecycleEmitter};
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::steering::{
    FollowUpQueueFull, MAX_APPLIED_FOLLOW_UP_INTENT_IDS, MAX_QUEUED_FOLLOW_UP_INTENTS, QueuedFollowUpIntent,
    SteeringMessage,
};
use crate::exec::events::{ThreadEvent, ToolCallStatus, ToolOutcome};
use crate::llm::provider::{
    AssistantPhase, FinishReason, LLMProvider, LLMRequest, LLMResponse, NormalizedStreamEvent, ToolCall,
    Usage as ProviderUsage,
};
use crate::llm::providers::gemini::wire::{Content, Part};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use std::collections::HashSet;
use std::collections::VecDeque;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::error::TryRecvError;

fn merge_stream_and_completed_text(accumulated: &mut String, completed: Option<&str>) {
    let Some(completed_text) = completed else {
        return;
    };
    if completed_text.is_empty() {
        return;
    }
    if accumulated.is_empty() {
        accumulated.push_str(completed_text);
        return;
    }
    if completed_text == accumulated.as_str() {
        return;
    }
    if let Some(suffix) = completed_text.strip_prefix(accumulated.as_str()) {
        accumulated.push_str(suffix);
        return;
    }
    accumulated.clear();
    accumulated.push_str(completed_text);
}

/// Control signal returned when polling the steering channel between turns or tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeControl {
    /// No steering message pending; execution may proceed normally.
    Continue,
    /// Execution was paused and has now been resumed.
    Resumed,
    /// A stop request was received; the current turn should be cancelled.
    StopRequested,
}

/// Progress event emitted by the model adapter during a streaming LLM response.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeModelProgress {
    /// A chunk of assistant output text.
    OutputDelta(String),
    /// A chunk of reasoning/thinking text.
    ReasoningDelta(String),
    /// The active reasoning stage label changed.
    ReasoningStage(String),
    /// A new tool call began streaming.
    ToolCallStarted {
        /// Identifier of the tool call.
        call_id: String,
        /// Optional name of the tool being invoked.
        name: Option<String>,
    },
    /// A chunk of arguments for an in-progress tool call.
    ToolCallDelta {
        /// Identifier of the tool call receiving the delta.
        call_id: String,
        /// Serialized argument fragment.
        delta: String,
    },
}

#[derive(Debug, Clone)]
struct RuntimeModelOutput {
    response: LLMResponse,
}

#[async_trait]
trait RuntimeModelAdapter {
    async fn execute(
        &mut self,
        request: LLMRequest,
        timeout: Option<std::time::Duration>,
        on_progress: &mut (dyn FnMut(RuntimeModelProgress) + Send),
    ) -> Result<RuntimeModelOutput>;
}

struct ProviderRuntimeModelAdapter<'a> {
    provider: &'a mut Box<dyn LLMProvider>,
    steering: &'a mut RuntimeSteering,
}

impl<'a> ProviderRuntimeModelAdapter<'a> {
    fn new(provider: &'a mut Box<dyn LLMProvider>, steering: &'a mut RuntimeSteering) -> Self {
        Self { provider, steering }
    }
}

#[async_trait]
impl RuntimeModelAdapter for ProviderRuntimeModelAdapter<'_> {
    async fn execute(
        &mut self,
        request: LLMRequest,
        timeout: Option<std::time::Duration>,
        on_progress: &mut (dyn FnMut(RuntimeModelProgress) + Send),
    ) -> Result<RuntimeModelOutput> {
        let request_model = request.model.clone();
        let mut stream = if let Some(duration) = timeout {
            match tokio::time::timeout(duration, self.provider.stream_normalized(request)).await {
                Ok(result) => result?,
                Err(_) => {
                    return Err(anyhow::anyhow!("Stream request timed out after {duration:?}"));
                }
            }
        } else {
            self.provider.stream_normalized(request).await?
        };

        let mut final_usage = ProviderUsage::default();
        let mut completed_response: Option<LLMResponse> = None;
        while let Some(event_result) = stream.next().await {
            if matches!(self.steering.poll_turn_control().await, RuntimeControl::StopRequested) {
                let mut response = LLMResponse {
                    model: request_model.clone(),
                    finish_reason: FinishReason::Error("Cancelled".to_string()),
                    usage: Some(final_usage.clone()),
                    ..Default::default()
                };
                if response.usage.as_ref().is_some_and(|usage| {
                    usage.prompt_tokens == 0 && usage.completion_tokens == 0 && usage.total_tokens == 0
                }) {
                    response.usage = None;
                }
                return Ok(RuntimeModelOutput { response });
            }

            match event_result? {
                NormalizedStreamEvent::TextDelta { delta } => {
                    on_progress(RuntimeModelProgress::OutputDelta(delta));
                }
                NormalizedStreamEvent::ReasoningDelta { delta } => {
                    on_progress(RuntimeModelProgress::ReasoningDelta(delta));
                }
                NormalizedStreamEvent::ToolCallStart { call_id, name } => {
                    on_progress(RuntimeModelProgress::ToolCallStarted { call_id, name });
                }
                NormalizedStreamEvent::ToolCallDelta { call_id, delta } => {
                    on_progress(RuntimeModelProgress::ToolCallDelta { call_id, delta });
                }
                NormalizedStreamEvent::Usage { usage } => {
                    final_usage = usage;
                }
                NormalizedStreamEvent::Done { response } => {
                    let mut response = *response;
                    if response.usage.is_none()
                        && (final_usage.prompt_tokens > 0
                            || final_usage.completion_tokens > 0
                            || final_usage.total_tokens > 0)
                    {
                        response.usage = Some(final_usage.clone());
                    }
                    completed_response = Some(response);
                    break;
                }
            }
        }

        let mut response = completed_response.unwrap_or_default();
        if response.model.is_empty() {
            response.model = request_model;
        }
        if response.usage.is_none()
            && (final_usage.prompt_tokens > 0 || final_usage.completion_tokens > 0 || final_usage.total_tokens > 0)
        {
            response.usage = Some(final_usage);
        }

        Ok(RuntimeModelOutput { response })
    }
}

/// Manages steering messages (stop, pause, resume, follow-up inputs) for a running agent turn.
pub struct RuntimeSteering {
    steering_receiver: Option<UnboundedReceiver<SteeringMessage>>,
    queued_follow_up_inputs: VecDeque<QueuedFollowUpIntent>,
    in_flight_follow_up_intents: VecDeque<QueuedFollowUpIntent>,
    applied_follow_up_intent_ids: VecDeque<String>,
}

impl Default for RuntimeSteering {
    fn default() -> Self {
        Self::new(None)
    }
}

impl RuntimeSteering {
    fn new(steering_receiver: Option<UnboundedReceiver<SteeringMessage>>) -> Self {
        Self {
            steering_receiver,
            queued_follow_up_inputs: VecDeque::new(),
            in_flight_follow_up_intents: VecDeque::new(),
            applied_follow_up_intent_ids: VecDeque::new(),
        }
    }

    /// Replace the steering message receiver channel.
    pub fn set_receiver(&mut self, receiver: Option<UnboundedReceiver<SteeringMessage>>) {
        self.steering_receiver = receiver;
    }

    /// Take ownership of the steering receiver, leaving `None` in its place.
    pub fn take_receiver(&mut self) -> Option<UnboundedReceiver<SteeringMessage>> {
        self.steering_receiver.take()
    }

    #[must_use]
    pub fn has_pending_follow_up_inputs(&self) -> bool {
        !self.queued_follow_up_inputs.is_empty()
    }

    /// Dequeue the next follow-up user input, if any are pending.
    pub fn pop_follow_up_input(&mut self) -> Option<String> {
        self.pop_follow_up_intent().map(|intent| intent.into_parts().1)
    }

    /// Dequeue the next identified follow-up intent.
    pub fn pop_follow_up_intent(&mut self) -> Option<QueuedFollowUpIntent> {
        self.queued_follow_up_inputs.pop_front()
    }

    /// Queue a follow-up user input. The boolean is `false` when the FIFO is
    /// full; callers that need the error detail should use the `try_` variant.
    pub fn queue_follow_up_input(&mut self, input: String) -> bool {
        match self.try_queue_follow_up_input(input) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "Rejected follow-up steering intent");
                false
            }
        }
    }

    /// Queue a follow-up user input and return an explicit overflow error.
    pub fn try_queue_follow_up_input(&mut self, input: String) -> Result<(), FollowUpQueueFull> {
        self.try_queue_follow_up_intent(QueuedFollowUpIntent::new(input))
    }

    /// Queue a recovered intent without changing its stable identity.
    pub fn try_queue_follow_up_intent(&mut self, intent: QueuedFollowUpIntent) -> Result<(), FollowUpQueueFull> {
        if self.queued_follow_up_inputs.len() + self.in_flight_follow_up_intents.len() >= MAX_QUEUED_FOLLOW_UP_INTENTS {
            return Err(FollowUpQueueFull { capacity: MAX_QUEUED_FOLLOW_UP_INTENTS });
        }
        self.queued_follow_up_inputs.push_back(intent);
        Ok(())
    }

    /// Mark an intent as applied after its tagged user message is stored.
    pub fn acknowledge_follow_up_intent(&mut self, intent_id: impl Into<String>) {
        if self.applied_follow_up_intent_ids.len() >= MAX_APPLIED_FOLLOW_UP_INTENT_IDS {
            self.applied_follow_up_intent_ids.pop_front();
        }
        self.applied_follow_up_intent_ids.push_back(intent_id.into());
    }

    #[must_use]
    pub fn applied_follow_up_intent_ids(&self) -> &VecDeque<String> {
        &self.applied_follow_up_intent_ids
    }

    #[must_use]
    pub fn pending_follow_up_intents(&self) -> &VecDeque<QueuedFollowUpIntent> {
        &self.queued_follow_up_inputs
    }

    /// Return all accepted intents that are not yet represented by durable
    /// session history, including the intent currently being processed.
    #[must_use]
    pub fn pending_follow_up_intents_snapshot(&self) -> Vec<QueuedFollowUpIntent> {
        self.queued_follow_up_inputs
            .iter()
            .chain(self.in_flight_follow_up_intents.iter())
            .cloned()
            .collect()
    }

    /// Mark intents whose tagged user messages have been durably checkpointed
    /// as applied. Until this is called, the intents remain in the pending
    /// snapshot so a crash cannot lose them between history and envelope IO.
    pub fn acknowledge_durable_follow_up_intents(&mut self) {
        while let Some(intent) = self.in_flight_follow_up_intents.pop_front() {
            self.acknowledge_follow_up_intent(intent.id());
        }
    }

    /// Release intents after their tagged messages have been applied when no
    /// history archive exists. This frees FIFO capacity for the in-process
    /// session without adding applied IDs that could falsely imply crash
    /// durability.
    pub fn release_in_flight_follow_up_intents_without_persistence(&mut self) {
        self.in_flight_follow_up_intents.clear();
    }

    pub fn clear_pending_follow_up_inputs(&mut self) {
        self.queued_follow_up_inputs.clear();
        self.in_flight_follow_up_intents.clear();
    }

    /// Poll the steering channel for control signals during the current turn.
    pub async fn poll_turn_control(&mut self) -> RuntimeControl {
        self.poll_control().await
    }

    /// Poll the steering channel for control signals during tool execution.
    pub async fn poll_tool_control(&mut self) -> RuntimeControl {
        self.poll_control().await
    }

    async fn poll_control(&mut self) -> RuntimeControl {
        let mut paused = false;

        loop {
            let Some(receiver) = self.steering_receiver.as_mut() else {
                return if paused {
                    RuntimeControl::Resumed
                } else {
                    RuntimeControl::Continue
                };
            };

            match receiver.try_recv() {
                Ok(SteeringMessage::SteerStop) => return RuntimeControl::StopRequested,
                Ok(SteeringMessage::Pause) => {
                    paused = true;
                    if matches!(self.wait_for_resume().await, RuntimeControl::StopRequested) {
                        return RuntimeControl::StopRequested;
                    }
                }
                Ok(SteeringMessage::Resume) => {
                    paused = true;
                }
                Ok(SteeringMessage::FollowUpInput(input)) => {
                    if let Err(error) = self.try_queue_follow_up_input(input) {
                        tracing::warn!(%error, "Rejected follow-up steering intent");
                    }
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                    return if paused {
                        RuntimeControl::Resumed
                    } else {
                        RuntimeControl::Continue
                    };
                }
            }
        }
    }

    async fn wait_for_resume(&mut self) -> RuntimeControl {
        loop {
            let Some(receiver) = self.steering_receiver.as_mut() else {
                return RuntimeControl::Continue;
            };

            match receiver.recv().await {
                Some(SteeringMessage::Resume) => return RuntimeControl::Continue,
                Some(SteeringMessage::SteerStop) => return RuntimeControl::StopRequested,
                Some(SteeringMessage::FollowUpInput(input)) => {
                    if let Err(error) = self.try_queue_follow_up_input(input) {
                        tracing::warn!(%error, "Rejected follow-up steering intent");
                    }
                }
                Some(SteeringMessage::Pause) => {}
                None => return RuntimeControl::Continue,
            }
        }
    }
}

/// Result of executing a single LLM turn, containing the full response and extracted fields.
pub struct TurnExecution {
    /// The complete LLM response including usage and finish reason.
    pub response: LLMResponse,
    /// The accumulated assistant output text.
    pub content: String,
    /// The accumulated reasoning/thinking text, if present.
    pub reasoning: Option<String>,
}

const MIN_REASONING_UPDATE_BYTES: usize = 256;
const MAX_REASONING_UPDATE_EVENTS: usize = 2;
const MIN_OUTPUT_UPDATE_BYTES: usize = 1024;
const MAX_OUTPUT_UPDATE_EVENTS: usize = 64;

/// Bridges streaming model progress events to the lifecycle event sink for real-time UI updates.
#[doc(hidden)]
pub struct StreamingLifecycleBridge {
    event_sink: Option<EventSink>,
    assistant_item_id: String,
    reasoning_item_id: String,
    lifecycle: SharedLifecycleEmitter,
    tool_call_item_ids: hashbrown::HashMap<String, String>,
    reasoning_stage: Option<String>,
    reasoning_update_events: usize,
    last_reasoning_emit_len: usize,
    output_update_events: usize,
    last_output_emit_len: usize,
}

impl StreamingLifecycleBridge {
    /// Create a new bridge that emits lifecycle events to the provided sink.
    #[must_use]
    pub fn new(event_sink: Option<EventSink>, turn_id: &str, step: usize, attempt: usize) -> Self {
        Self {
            event_sink,
            assistant_item_id: format!("{turn_id}-step-{step}-assistant-stream-{attempt}"),
            reasoning_item_id: format!("{turn_id}-step-{step}-reasoning-stream-{attempt}"),
            lifecycle: SharedLifecycleEmitter::default(),
            tool_call_item_ids: hashbrown::HashMap::new(),
            reasoning_stage: None,
            reasoning_update_events: 0,
            last_reasoning_emit_len: 0,
            output_update_events: 0,
            last_output_emit_len: 0,
        }
    }

    /// Forward a streaming progress event to the lifecycle emitter and sink.
    pub fn on_progress(&mut self, event: RuntimeModelProgress) {
        match event {
            RuntimeModelProgress::OutputDelta(delta) => self.push_assistant_delta(&delta),
            RuntimeModelProgress::ReasoningDelta(delta) => self.push_reasoning_delta(&delta),
            RuntimeModelProgress::ReasoningStage(stage) => self.update_reasoning_stage(stage),
            RuntimeModelProgress::ToolCallStarted { call_id, name } => {
                self.start_tool_call(call_id, name);
            }
            RuntimeModelProgress::ToolCallDelta { call_id, delta } => {
                self.push_tool_call_delta(call_id, &delta);
            }
        }
    }

    /// Abort the streaming turn, marking all open items as failed and flushing events.
    pub fn abort(&mut self) {
        self.lifecycle.complete_open_text_items();
        self.lifecycle.complete_open_tool_calls_with_status(ToolCallStatus::Failed);
        self.emit_pending_events();
    }

    /// Complete all open text items gracefully and flush pending events.
    pub fn complete_open_items(&mut self) {
        self.lifecycle.complete_open_text_items();
        self.emit_pending_events();
    }

    /// Take the mapping of tool call IDs to their lifecycle item IDs, leaving the map empty.
    #[must_use]
    pub fn take_streamed_tool_call_items(&mut self) -> hashbrown::HashMap<String, String> {
        std::mem::take(&mut self.tool_call_item_ids)
    }

    fn push_assistant_delta(&mut self, delta: &str) {
        if !self.lifecycle.append_assistant_delta(delta) {
            return;
        }

        if !self.should_emit_output_update() {
            return;
        }

        if self.lifecycle.emit_assistant_snapshot(Some(self.assistant_item_id.clone())) {
            self.output_update_events += 1;
            self.last_output_emit_len = self.lifecycle.assistant_len();
            self.emit_pending_events();
        }
    }

    fn push_reasoning_delta(&mut self, delta: &str) {
        if !self.lifecycle.append_reasoning_delta(delta) {
            return;
        }

        if !self.lifecycle.reasoning_started() {
            if self.lifecycle.emit_reasoning_snapshot(Some(self.reasoning_item_id.clone())) {
                self.last_reasoning_emit_len = self.lifecycle.reasoning_len();
                self.emit_pending_events();
            }
            return;
        }

        if !self.should_emit_reasoning_update(false) {
            return;
        }

        if self.lifecycle.emit_reasoning_snapshot(Some(self.reasoning_item_id.clone())) {
            self.record_reasoning_update();
            self.emit_pending_events();
        }
    }

    fn update_reasoning_stage(&mut self, stage: String) {
        let stage_changed = self.reasoning_stage.as_deref() != Some(stage.as_str());
        if !stage_changed || !self.lifecycle.set_reasoning_stage(Some(stage.clone())) {
            self.reasoning_stage = Some(stage);
            return;
        }
        self.reasoning_stage = Some(stage);

        if !self.lifecycle.reasoning_started() || !self.should_emit_reasoning_update(true) {
            return;
        }

        if self.lifecycle.emit_reasoning_stage_update() {
            self.record_reasoning_update();
            self.emit_pending_events();
        }
    }

    fn should_emit_reasoning_update(&self, stage_changed: bool) -> bool {
        if self.reasoning_update_events >= MAX_REASONING_UPDATE_EVENTS {
            return false;
        }

        stage_changed
            || self.lifecycle.reasoning_len().saturating_sub(self.last_reasoning_emit_len) >= MIN_REASONING_UPDATE_BYTES
    }

    fn record_reasoning_update(&mut self) {
        self.reasoning_update_events += 1;
        self.last_reasoning_emit_len = self.lifecycle.reasoning_len();
    }

    fn should_emit_output_update(&self) -> bool {
        if self.output_update_events >= MAX_OUTPUT_UPDATE_EVENTS {
            return false;
        }

        self.output_update_events == 0
            || self.lifecycle.assistant_len().saturating_sub(self.last_output_emit_len) >= MIN_OUTPUT_UPDATE_BYTES
    }

    fn start_tool_call(&mut self, call_id: String, name: Option<String>) {
        let item_id = format!("{}-tool-call-{call_id}", self.assistant_item_id);
        self.tool_call_item_ids.insert(call_id.clone(), item_id.clone());
        let _ = self.lifecycle.start_tool_call(&call_id, name, Some(item_id));
        self.emit_pending_events();
    }

    fn push_tool_call_delta(&mut self, call_id: String, delta: &str) {
        if !self.lifecycle.append_tool_call_delta(
            &call_id,
            delta,
            None,
            Some(format!("{}-tool-call-{call_id}", self.assistant_item_id)),
        ) {
            return;
        }
        self.emit_pending_events();
    }

    fn emit_pending_events(&mut self) {
        let Some(sink) = &self.event_sink else {
            let _ = self.lifecycle.drain_events();
            return;
        };

        for event in self.lifecycle.drain_events() {
            let mut callback = sink.lock();
            callback(&event);
        }
    }
}

/// Orchestrates a single agent turn: manages steering, lifecycle events, and the LLM call.
pub struct AgentRuntime {
    /// Mutable session state including conversation history and statistics.
    pub state: AgentSessionState,
    steering: RuntimeSteering,
    event_sink: Option<EventSink>,
    lifecycle: SharedLifecycleEmitter,
    emitted_events: Vec<ThreadEvent>,
    output_update_events: usize,
    last_output_emit_len: usize,
    reasoning_update_events: usize,
    last_reasoning_emit_len: usize,
}

impl AgentRuntime {
    /// Create a new runtime with the given session state, optional event sink, and steering receiver.
    pub fn new(
        state: AgentSessionState,
        event_sink: Option<EventSink>,
        steering_receiver: Option<UnboundedReceiver<SteeringMessage>>,
    ) -> Self {
        Self {
            state,
            steering: RuntimeSteering::new(steering_receiver),
            event_sink,
            lifecycle: SharedLifecycleEmitter::default(),
            emitted_events: Vec::new(),
            output_update_events: 0,
            last_output_emit_len: 0,
            reasoning_update_events: 0,
            last_reasoning_emit_len: 0,
        }
    }

    /// Replace the structured event sink used for lifecycle event emission.
    pub fn set_event_handler(&mut self, sink: Option<EventSink>) {
        self.event_sink = sink;
    }

    /// Replace the steering message receiver channel for this runtime.
    pub fn set_steering_receiver(&mut self, receiver: Option<UnboundedReceiver<SteeringMessage>>) {
        self.steering.set_receiver(receiver);
    }

    /// Take ownership of the steering receiver, leaving `None` in its place.
    pub fn take_steering_receiver(&mut self) -> Option<UnboundedReceiver<SteeringMessage>> {
        self.steering.take_receiver()
    }

    /// Borrow the session state and steering controller mutably at the same time.
    pub fn split_mut(&mut self) -> (&mut AgentSessionState, &mut RuntimeSteering) {
        (&mut self.state, &mut self.steering)
    }

    /// Returns `true` if there are queued follow-up user inputs waiting to be consumed.
    #[must_use]
    pub fn has_pending_follow_up_inputs(&self) -> bool {
        self.steering.has_pending_follow_up_inputs()
    }

    /// Pop the next queued follow-up input, add a tagged user message, and
    /// retain the intent until the enclosing history checkpoint succeeds.
    pub fn run_until_idle(&mut self) -> Option<String> {
        let intent = self.steering.pop_follow_up_intent()?;
        let (intent_id, input) = intent.into_parts();
        self.state.add_user_message_with_intent(input.clone(), Some(intent_id.clone()));
        self.steering
            .in_flight_follow_up_intents
            .push_back(QueuedFollowUpIntent::from_parts(intent_id, input.clone()));
        Some(input)
    }

    /// Queue a follow-up input to be consumed by `run_until_idle` on the next
    /// iteration, causing the session loop to start a new turn with this input
    /// without waiting for the user to type anything.
    pub fn queue_follow_up_input(&mut self, input: String) {
        let _ = self.steering.queue_follow_up_input(input);
    }

    /// Queue a follow-up input while exposing FIFO overflow to the caller.
    pub fn try_queue_follow_up_input(&mut self, input: String) -> Result<(), FollowUpQueueFull> {
        self.steering.try_queue_follow_up_input(input)
    }

    /// Restore the durable steering snapshot after loading session history.
    /// Pending intents already represented by a tagged user message, or
    /// already in the applied-ID window, are skipped; identical text with a
    /// different ID is intentionally retained.
    pub fn restore_follow_up_state(
        &mut self,
        pending_intents: impl IntoIterator<Item = QueuedFollowUpIntent>,
        applied_intent_ids: impl IntoIterator<Item = String>,
    ) -> Result<(), FollowUpQueueFull> {
        let mut applied = HashSet::new();
        for intent_id in applied_intent_ids {
            if applied.insert(intent_id.clone()) {
                self.steering.acknowledge_follow_up_intent(intent_id);
            }
        }
        let represented: HashSet<String> = self
            .state
            .messages
            .iter()
            .filter_map(|message| message.metadata.as_ref().and_then(|metadata| metadata.intent_id()))
            .map(ToOwned::to_owned)
            .collect();

        for intent in pending_intents {
            if applied.contains(intent.id()) || represented.contains(intent.id()) {
                continue;
            }
            self.steering.try_queue_follow_up_intent(intent)?;
        }
        Ok(())
    }

    /// Access the intent IDs acknowledged by this runtime for checkpointing.
    #[must_use]
    pub fn applied_follow_up_intent_ids(&self) -> &VecDeque<String> {
        self.steering.applied_follow_up_intent_ids()
    }

    /// Drop follow-up inputs belonging to the discarded execution context.
    pub fn clear_pending_follow_up_inputs(&mut self) {
        self.steering.clear_pending_follow_up_inputs();
    }

    /// Poll the steering channel for control signals during the current turn.
    pub async fn poll_turn_control(&mut self) -> RuntimeControl {
        self.steering.poll_turn_control().await
    }

    /// Poll the steering channel for control signals during tool execution.
    pub async fn poll_tool_control(&mut self) -> RuntimeControl {
        self.steering.poll_tool_control().await
    }

    /// Take all emitted lifecycle events, leaving the internal buffer empty.
    pub fn take_emitted_events(&mut self) -> Vec<ThreadEvent> {
        std::mem::take(&mut self.emitted_events)
    }

    /// Look up the lifecycle item ID for a given tool call identifier.
    #[must_use]
    pub fn tool_call_item_id(&self, call_id: &str) -> Option<String> {
        self.lifecycle.tool_call_item_id(call_id).map(str::to_string)
    }

    /// Mark a specific tool call as completed with the given status and emit lifecycle events.
    pub fn complete_tool_call(&mut self, call_id: &str, status: ToolCallStatus, outcome: Option<ToolOutcome>) {
        let _ = self.lifecycle.complete_tool_call(call_id, status, outcome);
        self.emit_pending_lifecycle_events();
    }

    /// Mark all still-open tool calls as completed with the given status and emit lifecycle events.
    pub fn complete_open_tool_calls(&mut self, status: ToolCallStatus) {
        self.lifecycle.complete_open_tool_calls_with_status(status);
        self.emit_pending_lifecycle_events();
    }

    fn emit_event(&mut self, event: ThreadEvent) {
        self.emitted_events.push(event.clone());
        if let Some(sink) = &self.event_sink {
            let mut callback = sink.lock();
            callback(&event);
        }
    }

    fn should_emit_output_update(&self) -> bool {
        if self.output_update_events >= MAX_OUTPUT_UPDATE_EVENTS {
            return false;
        }

        self.output_update_events == 0
            || self.lifecycle.assistant_len().saturating_sub(self.last_output_emit_len) >= MIN_OUTPUT_UPDATE_BYTES
    }

    fn should_emit_reasoning_update(&self) -> bool {
        if self.reasoning_update_events >= MAX_REASONING_UPDATE_EVENTS {
            return false;
        }

        self.reasoning_update_events == 0
            || self.lifecycle.reasoning_len().saturating_sub(self.last_reasoning_emit_len) >= MIN_REASONING_UPDATE_BYTES
    }

    fn emit_pending_lifecycle_events(&mut self) {
        for event in self.lifecycle.drain_events() {
            self.emit_event(event);
        }
    }

    fn finalize_assistant_lifecycle(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }

        let should_emit_snapshot = !self.lifecycle.assistant_started() || self.lifecycle.replace_assistant_text(text);
        if should_emit_snapshot {
            let _ = self.lifecycle.emit_assistant_snapshot(None);
        }
        let _ = self.lifecycle.complete_assistant_stream();
    }

    fn finalize_reasoning_lifecycle(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }

        let should_emit_snapshot = !self.lifecycle.reasoning_started() || self.lifecycle.replace_reasoning_text(text);
        if should_emit_snapshot {
            let _ = self.lifecycle.emit_reasoning_snapshot(None);
        }
        let _ = self.lifecycle.complete_reasoning_stream();
    }

    fn finalize_tool_call_lifecycle(&mut self, tool_calls: Option<&[ToolCall]>, _finish_reason: &str) {
        if let Some(tool_calls) = tool_calls {
            for call in tool_calls {
                let tool_name = call.function.as_ref().map(|function| function.name.clone());
                let _ = self.lifecycle.start_tool_call(&call.id, tool_name.clone(), None);
                if let Some(function) = call.function.as_ref() {
                    let _ = self
                        .lifecycle
                        .sync_tool_call_arguments(&call.id, &function.arguments, tool_name, None);
                }
            }
            return;
        }

        self.lifecycle.complete_open_tool_calls_with_status(ToolCallStatus::Failed);
    }

    fn record_model_progress(
        &mut self,
        event: RuntimeModelProgress,
        full_text: &mut String,
        full_reasoning: &mut String,
    ) {
        match event {
            RuntimeModelProgress::OutputDelta(delta) => {
                full_text.push_str(&delta);
                if self.lifecycle.append_assistant_delta(&delta) && self.should_emit_output_update() {
                    let _ = self.lifecycle.emit_assistant_snapshot(None);
                    self.output_update_events += 1;
                    self.last_output_emit_len = self.lifecycle.assistant_len();
                    self.emit_pending_lifecycle_events();
                }
            }
            RuntimeModelProgress::ReasoningDelta(delta) => {
                full_reasoning.push_str(&delta);
                if self.lifecycle.append_reasoning_delta(&delta) && self.should_emit_reasoning_update() {
                    let _ = self.lifecycle.emit_reasoning_snapshot(None);
                    self.reasoning_update_events += 1;
                    self.last_reasoning_emit_len = self.lifecycle.reasoning_len();
                    self.emit_pending_lifecycle_events();
                }
            }
            RuntimeModelProgress::ReasoningStage(stage) => {
                if self.lifecycle.set_reasoning_stage(Some(stage)) {
                    let _ = self.lifecycle.emit_reasoning_stage_update();
                    self.emit_pending_lifecycle_events();
                }
            }
            RuntimeModelProgress::ToolCallStarted { call_id, name } => {
                let _ = self.lifecycle.start_tool_call(&call_id, name, None);
                self.emit_pending_lifecycle_events();
            }
            RuntimeModelProgress::ToolCallDelta { call_id, delta } => {
                if self.lifecycle.append_tool_call_delta(&call_id, &delta, None, None) {
                    self.emit_pending_lifecycle_events();
                }
            }
        }
    }

    async fn run_turn_once_with_adapter<A: RuntimeModelAdapter + ?Sized>(
        &mut self,
        adapter: &mut A,
        request: LLMRequest,
        timeout: Option<std::time::Duration>,
    ) -> Result<TurnExecution> {
        let request_model = request.model.clone();
        let start_time = std::time::Instant::now();
        self.output_update_events = 0;
        self.last_output_emit_len = 0;
        self.reasoning_update_events = 0;
        self.last_reasoning_emit_len = 0;
        let mut full_text = String::new();
        let mut full_reasoning = String::new();
        let mut on_progress = |event| self.record_model_progress(event, &mut full_text, &mut full_reasoning);
        let RuntimeModelOutput { mut response } = adapter.execute(request, timeout, &mut on_progress).await?;

        merge_stream_and_completed_text(&mut full_text, response.content.as_deref());
        merge_stream_and_completed_text(&mut full_reasoning, response.reasoning.as_deref());

        let finish_reason = match response.finish_reason.clone() {
            FinishReason::Stop => "stop".to_string(),
            FinishReason::ToolCalls => "tool_calls".to_string(),
            FinishReason::Length => "length".to_string(),
            FinishReason::Error(message) => message,
            _ => "unknown".to_string(),
        };
        let final_usage = response.usage.take().unwrap_or_default();
        let mut aggregated_tool_calls = response.tool_calls.take();

        self.finalize_assistant_lifecycle(&full_text);
        self.finalize_reasoning_lifecycle(&full_reasoning);
        self.finalize_tool_call_lifecycle(aggregated_tool_calls.as_deref(), &finish_reason);
        self.emit_pending_lifecycle_events();

        let mut turn_recorded = false;
        self.state.record_turn(&start_time, &mut turn_recorded);

        if final_usage.prompt_tokens > 0 || final_usage.completion_tokens > 0 {
            self.state.stats.merge_usage(&final_usage);
        }

        aggregated_tool_calls = aggregated_tool_calls.filter(|calls| !calls.is_empty());

        let mut assistant_message = crate::llm::provider::Message::assistant(full_text.clone());
        if !full_reasoning.is_empty() {
            assistant_message = assistant_message.with_reasoning(Some(full_reasoning.clone()));
        }
        if let Some(details) = response.reasoning_details.take() {
            let values: Vec<serde_json::Value> = details.into_iter().map(serde_json::Value::String).collect();
            assistant_message = assistant_message.with_reasoning_details(Some(values));
        }

        match aggregated_tool_calls.as_ref() {
            Some(calls) => {
                assistant_message = assistant_message
                    .with_tool_calls(calls.clone())
                    .with_phase(Some(AssistantPhase::Commentary));
            }
            None => {
                assistant_message = assistant_message.with_phase(Some(AssistantPhase::FinalAnswer));
            }
        }

        self.state.adjust_token_count(assistant_message.estimate_tokens() as isize);
        self.state.messages_mut().push(assistant_message);

        self.state.conversation.push(Content {
            role: "model".to_string(),
            parts: vec![Part::Text { text: full_text.clone(), thought_signature: None }],
        });
        self.state.last_processed_message_idx = self.state.conversation.len();

        if response.model.is_empty() {
            response.model = request_model;
        }
        response.content = Some(full_text.clone());
        response.reasoning = if full_reasoning.is_empty() {
            None
        } else {
            Some(full_reasoning.clone())
        };
        response.tool_calls = aggregated_tool_calls;
        response.usage = Some(final_usage);
        response.finish_reason = if finish_reason == "tool_calls" {
            FinishReason::ToolCalls
        } else if finish_reason == "Cancelled" || finish_reason == "cancelled" {
            FinishReason::Error("Cancelled".to_string())
        } else {
            response.finish_reason
        };

        Ok(TurnExecution {
            response,
            content: full_text,
            reasoning: if full_reasoning.is_empty() {
                None
            } else {
                Some(full_reasoning)
            },
        })
    }

    /// Execute a single LLM turn: stream the response, update session state, and return the result.
    pub async fn run_turn_once(
        &mut self,
        provider: &mut Box<dyn LLMProvider>,
        request: LLMRequest,
        timeout: Option<std::time::Duration>,
    ) -> Result<TurnExecution> {
        let mut steering = std::mem::take(&mut self.steering);
        let mut adapter = ProviderRuntimeModelAdapter::new(provider, &mut steering);
        let result = self.run_turn_once_with_adapter(&mut adapter, request, timeout).await;
        self.steering = steering;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;

    use crate::llm::provider::{LLMError, LLMNormalizedStream, LLMStream, LLMStreamEvent, NormalizedStreamEvent};

    #[derive(Clone)]
    struct CompletedOnlyStreamProvider {
        response: LLMResponse,
    }

    #[derive(Clone)]
    struct DeltaStreamProvider {
        response: LLMResponse,
        text_delta: String,
        reasoning_delta: String,
    }

    #[async_trait]
    impl LLMProvider for CompletedOnlyStreamProvider {
        fn name(&self) -> &str {
            "test-provider"
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(self.response.clone())
        }

        async fn stream(&self, _request: LLMRequest) -> Result<LLMStream, LLMError> {
            Ok(Box::pin(stream::iter(vec![Ok(LLMStreamEvent::Completed {
                response: Box::new(self.response.clone()),
            })])))
        }

        async fn stream_normalized(&self, _request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
            Ok(Box::pin(stream::iter(vec![Ok(NormalizedStreamEvent::Done {
                response: Box::new(self.response.clone()),
            })])))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["test-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    #[async_trait]
    impl LLMProvider for DeltaStreamProvider {
        fn name(&self) -> &str {
            "delta-provider"
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(self.response.clone())
        }

        async fn stream(&self, _request: LLMRequest) -> Result<LLMStream, LLMError> {
            Ok(Box::pin(stream::iter(vec![Ok(LLMStreamEvent::Completed {
                response: Box::new(self.response.clone()),
            })])))
        }

        async fn stream_normalized(&self, _request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
            Ok(Box::pin(stream::iter(vec![
                Ok(NormalizedStreamEvent::ReasoningDelta { delta: self.reasoning_delta.clone() }),
                Ok(NormalizedStreamEvent::TextDelta { delta: self.text_delta.clone() }),
                Ok(NormalizedStreamEvent::Done { response: Box::new(self.response.clone()) }),
            ])))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["test-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn queued_follow_up_inputs_are_applied_one_at_a_time() {
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut runtime = AgentRuntime::new(state, None, Some(receiver));

        sender
            .send(SteeringMessage::FollowUpInput("first".to_string()))
            .expect("first follow-up should queue");
        sender
            .send(SteeringMessage::FollowUpInput("second".to_string()))
            .expect("second follow-up should queue");

        assert_eq!(runtime.poll_turn_control().await, RuntimeControl::Continue);
        assert!(runtime.has_pending_follow_up_inputs());
        assert!(runtime.state.messages.is_empty());

        assert_eq!(runtime.run_until_idle().as_deref(), Some("first"));
        assert_eq!(
            runtime
                .state
                .messages
                .last()
                .map(|message| message.get_text_content().into_owned()),
            Some("first".to_string())
        );
        assert!(runtime.has_pending_follow_up_inputs());

        assert_eq!(runtime.run_until_idle().as_deref(), Some("second"));
        assert_eq!(
            runtime
                .state
                .messages
                .last()
                .map(|message| message.get_text_content().into_owned()),
            Some("second".to_string())
        );
        assert!(!runtime.has_pending_follow_up_inputs());
    }

    #[test]
    fn follow_up_queue_has_bounded_identity_preserving_fifo() {
        let mut steering = RuntimeSteering::default();
        for index in 0..MAX_QUEUED_FOLLOW_UP_INTENTS {
            assert!(steering.try_queue_follow_up_input(format!("input-{index}")).is_ok());
        }
        let error = steering
            .try_queue_follow_up_input("overflow".to_string())
            .expect_err("overflow must be reported");
        assert_eq!(error.capacity, MAX_QUEUED_FOLLOW_UP_INTENTS);

        let first = steering.pop_follow_up_intent().expect("first intent should exist");
        let second = steering.pop_follow_up_intent().expect("second intent should exist");
        assert_ne!(first.id(), second.id());
        assert_eq!(first.text(), "input-0");
        assert_eq!(second.text(), "input-1");
    }

    #[test]
    fn run_until_idle_tags_and_acknowledges_follow_up_intent() {
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let mut runtime = AgentRuntime::new(state, None, None);
        runtime
            .try_queue_follow_up_input("steer me".to_string())
            .expect("intent should be accepted");

        assert_eq!(runtime.run_until_idle().as_deref(), Some("steer me"));
        let message = runtime.state.messages.last().expect("tagged message should exist");
        let intent_id = message
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.intent_id())
            .expect("follow-up message should carry intent identity")
            .to_string();
        assert!(runtime.applied_follow_up_intent_ids().is_empty());
        assert_eq!(runtime.steering.pending_follow_up_intents_snapshot().len(), 1);
        runtime.steering.acknowledge_durable_follow_up_intents();
        assert_eq!(runtime.applied_follow_up_intent_ids().back(), Some(&intent_id));
    }

    #[test]
    fn recovery_replays_only_unrepresented_intent_ids() {
        let pending = QueuedFollowUpIntent::from_parts("intent-pending", "finish the request");
        let same_text_different_id = QueuedFollowUpIntent::from_parts("intent-distinct", pending.text());

        let mut interrupted =
            AgentRuntime::new(AgentSessionState::new("session".to_string(), 16, 4, 128_000), None, None);
        interrupted
            .restore_follow_up_state([pending.clone(), same_text_different_id.clone()], Vec::new())
            .expect("unwritten intents should be recoverable");
        assert_eq!(interrupted.steering.pending_follow_up_intents().len(), 2);
        assert_eq!(interrupted.steering.pending_follow_up_intents()[0], pending);
        assert_eq!(interrupted.steering.pending_follow_up_intents()[1], same_text_different_id);

        let mut restarted_state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        restarted_state.add_user_message_with_intent(pending.text().to_string(), Some(pending.id().to_string()));
        let mut restarted = AgentRuntime::new(restarted_state, None, None);
        restarted
            .restore_follow_up_state([pending.clone(), same_text_different_id], Vec::new())
            .expect("distinct pending intent should fit the recovery queue");

        assert_eq!(restarted.steering.pending_follow_up_intents().len(), 1);
        assert_eq!(restarted.steering.pending_follow_up_intents()[0].id(), "intent-distinct");
    }

    #[test]
    fn no_archive_release_reuses_follow_up_capacity_without_applied_ids() {
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let mut runtime = AgentRuntime::new(state, None, None);
        for index in 0..MAX_QUEUED_FOLLOW_UP_INTENTS {
            runtime
                .try_queue_follow_up_input(format!("input-{index}"))
                .expect("intent should be accepted");
        }

        for _ in 0..MAX_QUEUED_FOLLOW_UP_INTENTS {
            runtime.run_until_idle().expect("queued intent should become a user message");
        }
        assert!(runtime.try_queue_follow_up_input("overflow".to_string()).is_err());

        runtime.steering.release_in_flight_follow_up_intents_without_persistence();
        assert!(runtime.try_queue_follow_up_input("after-release".to_string()).is_ok());
        assert!(runtime.applied_follow_up_intent_ids().is_empty());
    }

    #[tokio::test]
    async fn paused_runtime_resumes_and_preserves_follow_up_inputs() {
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut runtime = AgentRuntime::new(state, None, Some(receiver));

        sender.send(SteeringMessage::Pause).expect("pause should send");
        sender
            .send(SteeringMessage::FollowUpInput("queued while paused".to_string()))
            .expect("follow-up should send");
        sender.send(SteeringMessage::Resume).expect("resume should send");

        assert_eq!(runtime.poll_turn_control().await, RuntimeControl::Resumed);
        assert!(runtime.has_pending_follow_up_inputs());
        assert_eq!(runtime.run_until_idle().as_deref(), Some("queued while paused"));
    }

    #[tokio::test]
    async fn paused_runtime_stop_request_wins() {
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut runtime = AgentRuntime::new(state, None, Some(receiver));

        sender.send(SteeringMessage::Pause).expect("pause should send");
        sender.send(SteeringMessage::SteerStop).expect("stop should send");

        assert_eq!(runtime.poll_turn_control().await, RuntimeControl::StopRequested);
        assert!(!runtime.has_pending_follow_up_inputs());
    }

    #[tokio::test]
    async fn run_turn_once_uses_completed_payload_when_no_deltas_exist() {
        let response = LLMResponse {
            content: Some("### Header\n- item".to_string()),
            model: "test-model".to_string(),
            finish_reason: FinishReason::Stop,
            reasoning: Some("**why** this works".to_string()),
            ..Default::default()
        };
        let provider = CompletedOnlyStreamProvider { response: response.clone() };
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let mut runtime = AgentRuntime::new(state, None, None);
        let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);
        let request = LLMRequest {
            model: "test-model".to_string(),
            ..Default::default()
        };

        let turn = runtime
            .run_turn_once(&mut provider_box, request, None)
            .await
            .expect("run_turn_once should succeed");

        assert_eq!(turn.content, "### Header\n- item");
        assert_eq!(turn.reasoning.as_deref(), Some("**why** this works"));
        assert_eq!(turn.response.content.as_deref(), Some("### Header\n- item"));
        assert_eq!(turn.response.reasoning.as_deref(), Some("**why** this works"));
    }

    #[tokio::test]
    async fn provider_runtime_model_adapter_emits_delta_progress() {
        let response = LLMResponse {
            content: Some("hello world".to_string()),
            model: "test-model".to_string(),
            finish_reason: FinishReason::Stop,
            reasoning: Some("trace".to_string()),
            ..Default::default()
        };
        let provider = DeltaStreamProvider {
            response,
            text_delta: "hello world".to_string(),
            reasoning_delta: "trace".to_string(),
        };
        let mut steering = RuntimeSteering::default();
        let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);
        let request = LLMRequest {
            model: "test-model".to_string(),
            ..Default::default()
        };

        let mut adapter = ProviderRuntimeModelAdapter::new(&mut provider_box, &mut steering);
        let mut seen_progress = Vec::new();
        let mut callback = |event| seen_progress.push(event);
        let output = adapter
            .execute(request, None, &mut callback)
            .await
            .expect("adapter execution should succeed");

        assert_eq!(output.response.content.as_deref(), Some("hello world"));
        assert_eq!(output.response.reasoning.as_deref(), Some("trace"));
        assert_eq!(
            seen_progress,
            vec![
                RuntimeModelProgress::ReasoningDelta("trace".to_string()),
                RuntimeModelProgress::OutputDelta("hello world".to_string()),
            ]
        );
    }

    #[test]
    fn streaming_lifecycle_bridge_tracks_tool_call_item_ids() {
        let mut bridge = StreamingLifecycleBridge::new(None, "turn_tool_map", 5, 2);
        bridge.on_progress(RuntimeModelProgress::ToolCallStarted {
            call_id: "call_42".to_string(),
            name: Some("shell".to_string()),
        });

        let item_ids = bridge.take_streamed_tool_call_items();
        assert_eq!(
            item_ids.get("call_42").map(String::as_str),
            Some("turn_tool_map-step-5-assistant-stream-2-tool-call-call_42")
        );
    }
}
