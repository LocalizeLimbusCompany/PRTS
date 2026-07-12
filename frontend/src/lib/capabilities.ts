import type { ProjectCapabilities } from '@/api/types'

/** Capability checks consume API truth only and never infer permissions from role names. */
export function hasProjectCapability(
  capabilities: ProjectCapabilities | null | undefined,
  capability: keyof ProjectCapabilities,
): boolean {
  return capabilities?.[capability] === true
}
