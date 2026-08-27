import test from "node:test";
import assert from "node:assert/strict";
import { CIAPRE_COLORS } from "../src/ciapre-theme.js";

function relativeLuminance(hex) {
  const channels = hex.slice(1).match(/../g).map((channel) => Number.parseInt(channel, 16) / 255);
  const linear = channels.map((channel) => channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
}

function contrast(foreground, background) {
  const light = relativeLuminance(foreground);
  const dark = relativeLuminance(background);
  return (Math.max(light, dark) + 0.05) / (Math.min(light, dark) + 0.05);
}

test("Ciapre text, controls, syntax, and status colors meet AA on their actual dark surfaces", () => {
  const normalText = [CIAPRE_COLORS.foreground, CIAPRE_COLORS.primary, CIAPRE_COLORS.secondary, CIAPRE_COLORS.muted, CIAPRE_COLORS.added, CIAPRE_COLORS.alertForeground];
  for (const foreground of normalText) {
    assert.ok(contrast(foreground, CIAPRE_COLORS.background) >= 4.5, `${foreground} on background`);
    assert.ok(contrast(foreground, CIAPRE_COLORS.surface) >= 4.5, `${foreground} on surface`);
  }
  for (const syntax of [CIAPRE_COLORS.keyword, CIAPRE_COLORS.string, CIAPRE_COLORS.number, CIAPRE_COLORS.comment, CIAPRE_COLORS.function, CIAPRE_COLORS.type, CIAPRE_COLORS.variable]) {
    assert.ok(contrast(syntax, CIAPRE_COLORS.background) >= 4.5, `${syntax} syntax token`);
  }
  assert.ok(contrast(CIAPRE_COLORS.alert, CIAPRE_COLORS.background) >= 4.5, "source alert on application background");

  const controlPairs = [
    ["primary", CIAPRE_COLORS.background, CIAPRE_COLORS.primary],
    ["approve", CIAPRE_COLORS.background, CIAPRE_COLORS.secondary],
    ["danger", CIAPRE_COLORS.alertForeground, CIAPRE_COLORS.surfaceRaised],
    ["subtle", CIAPRE_COLORS.foreground, CIAPRE_COLORS.surfaceRaised],
  ];
  for (const [control, foreground, background] of controlPairs) {
    assert.ok(contrast(foreground, background) >= 4.5, `${control} control`);
  }
});
