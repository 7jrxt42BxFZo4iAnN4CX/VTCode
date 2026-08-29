export interface WebMcpDeployment {
  readonly id: "chatgpt-site" | "github-pages";
  readonly label: string;
  readonly url: string;
  readonly origin: string;
}

export const WEBMCP_DEPLOYMENTS: readonly WebMcpDeployment[] = Object.freeze([
  {
    id: "chatgpt-site",
    label: "ChatGPT Site",
    url: "https://vtcode.vinhnx.chatgpt.site/",
    origin: "https://vtcode.vinhnx.chatgpt.site",
  },
  {
    id: "github-pages",
    label: "GitHub Pages",
    url: "https://vinhnx.github.io/VTCode/",
    origin: "https://vinhnx.github.io",
  },
]);

const LOCAL_WEBMCP_ORIGIN = "http://localhost:5173";

export function browserOrigin(
  location: Pick<Location, "origin"> | null | undefined = globalThis.location,
): string {
  const origin = location?.origin;
  return typeof origin === "string" && origin.length > 0 && origin !== "null"
    ? origin
    : LOCAL_WEBMCP_ORIGIN;
}

export function webmcpDeploymentForOrigin(origin: string): WebMcpDeployment | null {
  return WEBMCP_DEPLOYMENTS.find((deployment) => deployment.origin === origin) ?? null;
}
