import type { GraphicsPlatform } from "./graphicsSettings.ts";

/** What a host can observe about the device and connection. Every field is optional. */
export interface GraphicsPlatformSignals {
  /** The host is a native shell. This decides the platform on its own. */
  readonly native?: boolean;
  /** `navigator.userAgentData.mobile`, where the browser provides it. */
  readonly mobile?: boolean;
  readonly userAgent?: string;
  /** The primary pointer is coarse: `matchMedia("(pointer: coarse)")`. */
  readonly coarsePointer?: boolean;
  readonly maxTouchPoints?: number;
  /** `navigator.deviceMemory` in GiB. Browsers report at most 8. */
  readonly deviceMemory?: number;
  /** The GPU's `MAX_TEXTURE_SIZE`, when the host has a context to ask. */
  readonly maxTextureSize?: number;
  /** `navigator.connection.type`, such as `cellular` or `wifi`. */
  readonly connectionType?: string;
  /** `navigator.connection.effectiveType`: `slow-2g`, `2g`, `3g` or `4g`. */
  readonly effectiveType?: string;
  /** `navigator.connection.saveData`, or the player's own data-saving choice. */
  readonly saveData?: boolean;
}

const PHONE_AGENT = /iPhone|iPod|Android.+Mobile|Windows Phone/i;
const SLOW_CONNECTIONS = ["slow-2g", "2g", "3g"];
const DESKTOP_MEMORY_GB = 8, DESKTOP_TEXTURE_SIZE = 16384;

/**
 * Infer a platform profile from stated signals. It reads nothing from the environment.
 *
 * - A native host is `native-pc`.
 * - A phone is a mobile user agent, or a coarse primary pointer with touch points. A tablet,
 *   an iPad included, reads as a phone.
 * - A phone is on cellular when it says so, when its connection is slower than 4G, or when
 *   data saving is on. A 4G effective type does not show it, because Wi-Fi reports the same.
 *   A phone that reports nothing about its connection, as every iPhone does, is `phone-wifi`.
 * - Other devices are `desktop-web` unless they show less than 8 GiB of memory or a texture
 *   limit under 16384, which makes them `laptop-web`. A browser that reports neither is
 *   `desktop-web`, the profile without caps.
 */
export function detectGraphicsPlatform(signals: GraphicsPlatformSignals = {}): GraphicsPlatform {
  if (signals.native) return "native-pc";
  const phone = signals.mobile === true || PHONE_AGENT.test(signals.userAgent ?? "")
    || (signals.coarsePointer === true && (signals.maxTouchPoints ?? 0) > 0);
  if (phone) {
    const cellular = signals.saveData === true || signals.connectionType === "cellular"
      || SLOW_CONNECTIONS.includes(signals.effectiveType ?? "");
    return cellular ? "phone-cellular" : "phone-wifi";
  }
  const small = (signals.deviceMemory !== undefined && signals.deviceMemory < DESKTOP_MEMORY_GB)
    || (signals.maxTextureSize !== undefined && signals.maxTextureSize < DESKTOP_TEXTURE_SIZE);
  return small ? "laptop-web" : "desktop-web";
}

/**
 * Gather the signals this browser exposes. `stated` adds what only the host knows, such as
 * `native`, `maxTextureSize` from its own context, or the player's data-saving choice, and wins
 * over what was read. Outside a browser it returns `stated` alone.
 */
export function graphicsPlatformSignals(stated: GraphicsPlatformSignals = {}): GraphicsPlatformSignals {
  const browser = globalThis.navigator as (Navigator & {
    userAgentData?: { mobile?: boolean };
    deviceMemory?: number;
    connection?: { type?: string; effectiveType?: string; saveData?: boolean };
  }) | undefined;
  if (!browser) return stated;
  const read: GraphicsPlatformSignals = {
    mobile: browser.userAgentData?.mobile,
    userAgent: browser.userAgent,
    coarsePointer: globalThis.matchMedia?.("(pointer: coarse)").matches,
    maxTouchPoints: browser.maxTouchPoints,
    deviceMemory: browser.deviceMemory,
    connectionType: browser.connection?.type,
    effectiveType: browser.connection?.effectiveType,
    saveData: browser.connection?.saveData,
  };
  const known = (signals: GraphicsPlatformSignals) => Object.fromEntries(Object.entries(signals).filter(([, value]) => value !== undefined));
  return { ...known(read), ...known(stated) };
}

/** The platform this browser appears to be. Pass the result as `resolveRenderProfile`'s `platform` option to opt in. */
export function detectBrowserGraphicsPlatform(stated?: GraphicsPlatformSignals): GraphicsPlatform {
  return detectGraphicsPlatform(graphicsPlatformSignals(stated));
}
