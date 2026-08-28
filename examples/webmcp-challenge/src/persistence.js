const STORAGE_KEY = "vtcode-webmcp:browser-workspace";
const SETTINGS_STORAGE_KEY = "vtcode-webmcp:settings";
const STORAGE_VERSION = 1;
const SETTINGS_VERSION = 1;
const MAX_STATE_BYTES = 8 * 1024 * 1024;

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function validText(value, maxBytes = 2 * 1024 * 1024) {
  return typeof value === "string" && new TextEncoder().encode(value).length <= maxBytes;
}

function validPath(path) {
  return typeof path === "string" && path.length > 0 && path.length <= 4096 && !path.includes("\0") && !path.startsWith("/") && !path.split("/").includes("..");
}

function validBridgeUrl(value) {
  if (typeof value !== "string" || value.length === 0 || value.length > 2048) return false;
  try {
    const url = new URL(value);
    return (url.protocol === "ws:" || url.protocol === "wss:")
      && !url.username
      && !url.password
      && !url.search
      && !url.hash;
  } catch {
    return false;
  }
}

function sanitizeFiles(value) {
  if (!isObject(value)) return {};
  return Object.fromEntries(Object.entries(value).filter(([path, content]) => validPath(path) && validText(content)));
}

function sanitizeState(value) {
  if (!isObject(value) || value.version !== STORAGE_VERSION || typeof value.app_instance !== "string") return null;
  const paths = (items) => Array.isArray(items) ? items.filter(validPath).slice(0, 256) : [];
  return {
    version: STORAGE_VERSION,
    app_instance: value.app_instance,
    fallback_files: sanitizeFiles(value.fallback_files),
    drafts: sanitizeFiles(value.drafts),
    open_tabs: paths(value.open_tabs),
    selected: validPath(value.selected) ? value.selected : null,
    expanded_dirs: paths(value.expanded_dirs),
    filter: typeof value.filter === "string" ? value.filter.slice(0, 160) : "",
    workspace_path: typeof value.workspace_path === "string" ? value.workspace_path.slice(0, 4096) : "",
  };
}

export function loadBrowserState(storage, appInstance) {
  if (!storage || typeof appInstance !== "string" || !appInstance) return null;
  try {
    const raw = storage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const state = sanitizeState(JSON.parse(raw));
    if (!state || state.app_instance !== appInstance) {
      storage.removeItem(STORAGE_KEY);
      return null;
    }
    return state;
  } catch {
    return null;
  }
}

export function saveBrowserState(storage, appInstance, state) {
  if (!storage || typeof appInstance !== "string" || !appInstance || !isObject(state)) return false;
  const value = {
    version: STORAGE_VERSION,
    app_instance: appInstance,
    fallback_files: state.fallback_files,
    drafts: state.drafts,
    open_tabs: state.open_tabs,
    selected: state.selected,
    expanded_dirs: state.expanded_dirs,
    filter: state.filter,
    workspace_path: state.workspace_path,
  };
  try {
    const serialized = JSON.stringify(value);
    if (new TextEncoder().encode(serialized).length > MAX_STATE_BYTES) return false;
    storage.setItem(STORAGE_KEY, serialized);
    return true;
  } catch {
    return false;
  }
}

function sanitizeSettings(value) {
  if (!isObject(value) || value.version !== SETTINGS_VERSION || typeof value.app_instance !== "string") return null;
  return {
    version: SETTINGS_VERSION,
    app_instance: value.app_instance,
    workspace_path: typeof value.workspace_path === "string" ? value.workspace_path.slice(0, 4096) : "",
    bridge_url: validBridgeUrl(value.bridge_url) ? value.bridge_url : "",
  };
}

export function loadBrowserSettings(storage, appInstance) {
  if (!storage || typeof appInstance !== "string" || !appInstance) return null;
  try {
    const raw = storage.getItem(SETTINGS_STORAGE_KEY);
    if (!raw) return null;
    const settings = sanitizeSettings(JSON.parse(raw));
    if (!settings || settings.app_instance !== appInstance) {
      storage.removeItem(SETTINGS_STORAGE_KEY);
      return null;
    }
    return settings;
  } catch {
    return null;
  }
}

export function saveBrowserSettings(storage, appInstance, settings) {
  if (!storage || typeof appInstance !== "string" || !appInstance || !isObject(settings)) return false;
  const value = {
    version: SETTINGS_VERSION,
    app_instance: appInstance,
    workspace_path: typeof settings.workspace_path === "string" ? settings.workspace_path.slice(0, 4096) : "",
    bridge_url: validBridgeUrl(settings.bridge_url) ? settings.bridge_url : "",
  };
  try {
    storage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(value));
    return true;
  } catch {
    return false;
  }
}

export const BROWSER_WORKSPACE_STORAGE_KEY = STORAGE_KEY;
export const BROWSER_SETTINGS_STORAGE_KEY = SETTINGS_STORAGE_KEY;
