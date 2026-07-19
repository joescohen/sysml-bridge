import { createHash } from "node:crypto";

/**
 * Deterministic stable ID: sha256 over `${namespace}:${naturalKey}`, hex, truncated.
 *
 * This is a content-addressing hash for deterministic identity — NOT a security
 * hash or MAC guarantee (no key, not collision-resistant at cryptographic security
 * levels). Intended use: stable, row-order-independent IDs for corpus-extracted
 * MBSE entities so re-extraction never churns IDs (IR-02).
 *
 * Returns `${namespace}-${hex.slice(0, 16)}` — 16 hex chars / 64 bits, which is
 * collision-safe for low-thousands corpus entities per RESEARCH A2.
 *
 * Pure function — no I/O, no randomness, no call-order dependence.
 */
export function stableId(namespace: string, naturalKey: string): string {
  const input = `${namespace}:${naturalKey}`;
  const hex = createHash("sha256").update(input, "utf8").digest("hex");
  return `${namespace}-${hex.slice(0, 16)}`;
}
