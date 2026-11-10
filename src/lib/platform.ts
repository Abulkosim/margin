export type Platform = 'mac' | 'windows'

/** Margin ships for macOS and Windows; any other browser previews the macOS app. */
export function platformOf(userAgent: string): Platform {
  return userAgent.includes('Windows') ? 'windows' : 'mac'
}

export const platform: Platform = platformOf(globalThis.navigator?.userAgent ?? '')
export const isMac = platform === 'mac'
export const isWindows = platform === 'windows'
