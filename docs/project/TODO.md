https://github.com/EmbarkStudios/crash-handling

https://github.com/EmbarkStudios/cargo-about

===

adding a new `clock` or `time` to the /statusline config, so that in the fullscreen alternative transcript log, we can always see the current system date time. make it the default config and visible in the statusline. this is useful for debugging and for logging purposes, especially when we are running vtcode in fullscreen mode and we want to know the current time.

===

TIL that #[serde(flatten)] can have meaningful overhead, since all fields not explicitly declared on the outer struct are buffered

https://github.com/astral-sh/uv/pull/20881
