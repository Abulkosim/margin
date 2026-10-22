import { z } from 'zod'

export const providerSchema = z.enum(['claude', 'codex'])
export const accountIdSchema = z.string().min(1).brand<'AccountId'>()
export const settingsSchema = z.object({
  theme: z.enum(['system', 'light', 'dark']),
  pollIntervalSeconds: z.number().int().min(30).max(3600),
  idlePollIntervalSeconds: z.number().int().min(30).max(86400),
  notifications: z.boolean(),
  thresholds: z.array(z.number().int().min(1).max(100)).min(1).max(10)
    .refine(values => values.every((value, index) => index === 0 || value > (values[index - 1] ?? 0)), 'Thresholds must be distinct and increasing'),
  notifyOnReset: z.boolean().default(false),
  notificationPermission: z.enum(['unasked', 'granted', 'denied']).default('unasked'),
  trayMode: z.enum(['pinned', 'lowest']),
  trayMetric: z.enum(['core', 'all', 'session', 'weekly']).default('core'),
  trayShowPercent: z.boolean().default(false),
  pinnedAccountId: accountIdSchema.nullable(),
  autostart: z.boolean(),
  autoCheckUpdates: z.boolean().default(true),
}).refine(value => value.idlePollIntervalSeconds >= value.pollIntervalSeconds, 'Background refresh must be slower than active refresh')
export const usageWindowSchema = z.object({
  id: z.string(),
  label: z.string(),
  usedPercent: z.number().min(0).max(100),
  resetsAt: z.string().datetime({ offset: true }).nullable(),
  windowSeconds: z.number().int().nonnegative().nullable(),
  scope: z.enum(['core', 'model', 'other']).optional(),
})
export const resetGrantsSchema = z.object({
  available: z.number().int().nonnegative(),
  kind: z.string().nullable(),
  expiresAt: z.string().datetime({ offset: true }).nullable(),
})
export const accountSchema = z.object({
  id: accountIdSchema,
  provider: providerSchema,
  label: z.string(),
  color: z.string(),
  source: z.discriminatedUnion('kind', [
    z.object({ kind: z.literal('configDir'), path: z.string(), keychainSuffix: z.string().optional() }),
    z.object({ kind: z.literal('keychain') }),
  ]),
  identity: z.object({ email: z.string().nullable(), plan: z.string().nullable() }),
  snapshot: z.object({
    windows: z.array(usageWindowSchema),
    fetchedAt: z.string().datetime({ offset: true }),
    extraUsage: z.object({
      spent: z.number().nonnegative(),
      limit: z.number().nonnegative().nullable(),
      currency: z.string(),
    }).nullable(),
    resetGrants: resetGrantsSchema.nullable().default(null),
  }).nullable(),
  error: z.object({
    kind: z.string(),
    message: z.string(),
    retryAfterSeconds: z.number().nonnegative().nullable(),
    signedOutSince: z.string().datetime({ offset: true }).optional(),
    signInCommand: z.string().optional(),
    retryAt: z.number().int().nonnegative().optional(),
  }).nullable(),
  refreshing: z.boolean(),
})
export const appStateSchema = z.object({
  revision: z.number().int().nonnegative(),
  accounts: z.array(accountSchema),
  settings: settingsSchema,
  refreshing: z.boolean(),
  notice: z.string().nullable().default(null),
  recovery: z.boolean().default(false),
  recovered: z.array(z.string()).default([]),
  ignoredSources: z.array(z.string()).default([]),
})

export type Provider = z.infer<typeof providerSchema>
export type AccountId = z.infer<typeof accountIdSchema>
export type Account = z.infer<typeof accountSchema>
export type Settings = z.infer<typeof settingsSchema>
export type UsageWindow = z.infer<typeof usageWindowSchema>
export type ResetGrants = z.infer<typeof resetGrantsSchema>
export type WindowScope = NonNullable<UsageWindow['scope']>
export type AppState = z.infer<typeof appStateSchema>

export const defaultSettings: Settings = {
  theme: 'system',
  pollIntervalSeconds: 60,
  idlePollIntervalSeconds: 300,
  notifications: true,
  thresholds: [80, 95, 100],
  notifyOnReset: false,
  notificationPermission: 'unasked',
  trayMode: 'pinned',
  trayMetric: 'core',
  trayShowPercent: false,
  pinnedAccountId: null,
  autostart: false,
  autoCheckUpdates: true,
}
