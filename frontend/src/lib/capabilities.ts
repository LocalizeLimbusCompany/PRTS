import type { PlatformCapabilities, ProjectCapabilities } from '@/api/types'

/** Capability checks consume API truth only and never infer permissions from role names. */
export function hasProjectCapability(
  capabilities: ProjectCapabilities | null | undefined,
  capability: keyof ProjectCapabilities,
): boolean {
  return capabilities?.[capability] === true
}

/** Platform capability checks also consume explicit API truth only. */
export function hasPlatformCapability(
  capabilities: PlatformCapabilities | null | undefined,
  capability: keyof PlatformCapabilities,
): boolean {
  return capabilities?.[capability] === true
}
