---
phase: code-review
reviewed: 2026-05-05T00:00:00Z
depth: standard
files_reviewed: 10
files_reviewed_list:
  - dashboard/src/App.tsx
  - dashboard/src/components/DiagramPanel.tsx
  - dashboard/src/components/ProjectDetail.tsx
  - dashboard/src/lib/__tests__/diagram-generators.test.ts
  - dashboard/src/lib/diagram-generators.ts
  - dashboard/src/lib/ibd-transformer.ts
  - dashboard/src/types/sysml.ts
  - docker/docker-compose.yml
  - packages/skills/skills/mbse-init.md
  - packages/skills/skills/mbse-requirements.md
findings:
  critical: 1
  warning: 6
  info: 4
  total: 11
status: issues_found
---

# Code Review Report

**Reviewed:** 2026-05-05
**Depth:** standard
**Files Reviewed:** 10
**Status:** issues_found

## Summary

Reviewed the full set of changed files spanning the React dashboard, diagram generation logic, IBD transformer, type definitions, Docker infrastructure stub, and MBSE skills. The codebase is well-structured overall with good cancellation patterns in async effects and solid defensive programming in the graph algorithms.

The critical finding is an XSS/content-injection risk in the SysON iframe where the `sysonBase` URL is taken from an environment variable without any validation, and is concatenated directly with server-controlled IDs into an iframe `src`. Several warnings cover logic bugs in tab state, a silent data race in `DiagramPanel`, an incorrect test assertion, and edge cases in the graph algorithms. Four informational items round out naming, dead-code, and documentation gaps.

---

## Critical Issues

### CR-01: Unsanitized `VITE_SYSON_URL` injected into iframe src

**File:** `dashboard/src/components/DiagramPanel.tsx:90,149`

**Issue:** `sysonBase` is derived from `import.meta.env.VITE_SYSON_URL` with no validation and then concatenated directly into an iframe `src`:

```ts
const sysonBase = import.meta.env.VITE_SYSON_URL ?? 'http://localhost:8080';
// ...
src={`${sysonBase}/projects/${projectId}/edit/${activeSysOnTab_.repId}`}
```

If an attacker or misconfigured environment provides a `javascript:` URI, `data:` URL, or a redirector as `VITE_SYSON_URL`, the iframe will load attacker-controlled content inside the application origin. Additionally, `projectId` and `repId` come from the server API response (`Representation.id` and `Project['@id']`) and are interpolated without encoding — if the API is compromised or returns crafted IDs containing `?` or `#` fragments, the navigation target can be altered.

**Fix:** Validate the base URL at startup and encode the path segments:

```ts
function buildSysONUrl(base: string, projectId: string, repId: string): string | null {
  try {
    const origin = new URL(base);
    // Only allow http/https origins
    if (origin.protocol !== 'http:' && origin.protocol !== 'https:') return null;
    // Encode each path segment individually
    const path = [
      'projects',
      encodeURIComponent(projectId),
      'edit',
      encodeURIComponent(repId),
    ].join('/');
    return `${origin.origin}/${path}`;
  } catch {
    return null;
  }
}

// In component:
const iframeSrc = buildSysONUrl(sysonBase, projectId, activeSysOnTab_.repId);
// Render only if valid:
{iframeSrc && <iframe src={iframeSrc} ... />}
```

---

## Warnings

### WR-01: Silent data race — `getRepresentations` result can overwrite stale state

**File:** `dashboard/src/components/DiagramPanel.tsx:25-29`

**Issue:** The `useEffect` fires `getRepresentations(projectId).then(setRepresentations)` but unlike `ProjectDetail.tsx`'s `getElements` call, it has no cancellation guard. If `projectId` changes quickly (e.g., user clicks another project before the first fetch resolves), the stale response from the first fetch will call `setRepresentations([...])` after the component has already switched to showing a different project, populating the diagram tab bar with the wrong project's diagrams.

**Fix:** Mirror the cancellation pattern used in `ProjectDetail`:

```ts
useEffect(() => {
  let cancelled = false;
  setActiveSysOnTab(0);
  setActiveGenTab(0);
  getRepresentations(projectId)
    .then(r => { if (!cancelled) setRepresentations(r); })
    .catch(() => { if (!cancelled) setRepresentations([]); });
  return () => { cancelled = true; };
}, [projectId, refreshKey]);
```

### WR-02: `activeSysOnTab` and `activeGenTab` are not reset when switching sources

**File:** `dashboard/src/components/DiagramPanel.tsx:116-127`

**Issue:** When the user clicks the "SysON" or "Generated" source toggle, `onClearElementNav?.()` and `setSource(s)` are called, but neither `setActiveSysOnTab(0)` nor `setActiveGenTab(0)` is reset. The `safeGen` / `safeSysOn` clamp logic prevents an out-of-bounds access, but it means switching from Generated (tab 3 selected) back to SysON leaves the stored index at 3 — when the user switches back to Generated the tab jumps to whatever index 3 resolves to, not back to tab 0.

**Fix:**

```ts
onClick={() => {
  onClearElementNav?.();
  setSource(s);
  setActiveSysOnTab(0);
  setActiveGenTab(0);
}}
```

### WR-03: `RequirementCard` always passes `depth={1}` for children (recursion broken)

**File:** `dashboard/src/components/DiagramPanel.tsx:304-307`

**Issue:** `RequirementCard` is a recursive component, but recursive calls pass a hard-coded `depth={1}` instead of `depth + 1`:

```ts
{node.children.map(child => (
  <RequirementCard key={child.id} node={child} depth={1} />  // BUG: should be depth + 1
))}
```

This means requirements nested more than two levels deep all render at the same indent (`1 * 20 = 20px`) instead of increasing indentation, making deep hierarchies visually flat and indistinguishable from second-level nodes.

**Fix:**

```ts
{node.children.map(child => (
  <RequirementCard key={child.id} node={child} depth={depth + 1} />
))}
```

### WR-04: Test assertion for `root.childIds` is semantically wrong

**File:** `dashboard/src/lib/__tests__/diagram-generators.test.ts:19-25`

**Issue:** The test comment says "only check id/name, not childIds (which is for backward compat only)" and does not assert `childIds`, which is correct for intent. However, `buildBDDModel` constructs the legacy root as:

```ts
root: legacy ? { id: legacy.id, name: legacy.name, childIds: legacy.parts.map(() => '') } : undefined,
```

This sets `childIds` to an array of empty strings (one per part), not actual child block IDs. Any consumer relying on `root.childIds` to traverse the hierarchy will silently get garbage. The test does not cover this, so the bug goes undetected.

**Fix:** Either populate `childIds` correctly (using `legacy.childIds` from the block) or remove the field since the test already documents it as "backward compat only":

```ts
// Option A — correct population:
root: legacy ? { id: legacy.id, name: legacy.name, childIds: legacy.childIds } : undefined,

// Option B — remove the misleading field:
root: legacy ? { id: legacy.id, name: legacy.name } : undefined,
// (and update BDDModel interface accordingly)
```

Add a test to catch regressions:
```ts
expect(model.root?.childIds).not.toContain('');
```

### WR-05: `buildActivityModel` edge resolution silently promotes unresolved endpoints to sentinel nodes

**File:** `dashboard/src/lib/diagram-generators.ts:391-393`

**Issue:** When building activity edges, if `fromId` or `toId` is absent (i.e., no source/target ref), the code falls back to the sentinel start/end nodes:

```ts
const from = fromId && nodeIds.has(fromId) ? fromId : (fromId ? undefined : startId);
const to   = toId   && nodeIds.has(toId)   ? toId   : (toId   ? undefined : endId);
```

The condition `(fromId ? undefined : startId)` means: if `fromId` is present but not in `nodeIds`, return `undefined` (edge dropped — correct). But if `fromId` is absent entirely, it returns `startId`. This silently creates an edge from the sentinel start node to any action that has no explicit predecessor. For actions that are legitimately mid-chain but whose predecessor was filtered out (e.g., owned by a different `ActionDefinition`), this will draw a phantom edge from `start → action`, producing incorrect diagrams without any warning.

**Fix:** Only fall back to sentinels intentionally (e.g., for edges whose source/target is the `ActionDefinition` itself), not for all absent refs. At minimum, require both endpoints to resolve:

```ts
const from = fromId && nodeIds.has(fromId) ? fromId : undefined;
const to   = toId   && nodeIds.has(toId)   ? toId   : undefined;
if (from && to) edges.push({ fromId: from, toId: to, label: elementName(e) || undefined });
```

### WR-06: `ibd-transformer.ts` calls `resolveLogicalOwner` twice per port in the nodes-building loop (O(n) duplicated work with silent staleness risk)

**File:** `dashboard/src/lib/ibd-transformer.ts:93-95`

**Issue:** When constructing React Flow nodes, `blockPorts` is computed by filtering all `portUsages` and calling `resolveLogicalOwner(p, byId)` inside the filter callback for every block:

```ts
const blockPorts = portUsages.filter(p => resolveLogicalOwner(p, byId) === block['@id']);
```

This iterates `portUsages` once per block and calls `resolveLogicalOwner` on every port for every block. Since `resolveLogicalOwner` itself walks the ownership chain (potentially multiple hops through membership nodes), the total work is O(blocks × ports × chain_depth). This is a correctness concern as well: the `portToBlock` map was already built earlier in the function (lines 62-71) using the exact same `resolveLogicalOwner` logic, but that result is not reused here. If the two walks produce different results (e.g., due to differing traversal of the same chain), a port could appear in the `portToBlock` map for block A but be placed visually on block B's node.

**Fix:** Reuse the already-computed `portToBlock` map:

```ts
const blockPorts = portUsages.filter(p => portToBlock.get(p['@id']) === block['@id']);
```

This guarantees consistency between the edge-routing logic (which uses `portToBlock`) and the visual port placement.

---

## Info

### IN-01: `docker-compose.yml` is a comment-only stub — misleading filename

**File:** `docker/docker-compose.yml:1-11`

**Issue:** The file contains only comments explaining that Docker runs on a remote Windows machine and provides no actual Compose service definitions. Anyone running `docker compose up` from this directory will get a valid-but-empty compose file (no services). A new contributor would reasonably expect this file to define the stack.

**Fix:** Either rename it to `docker/README.md` / `docker/INFRASTRUCTURE.md` to signal it is documentation, or add a comment at the top explicitly marking it as a placeholder stub:

```yaml
# PLACEHOLDER — no local services. See comment below for remote setup.
```

### IN-02: `gradFor` hash uses `& 0xfffff` (20 bits) but should use `& 0xffffffff` for JS 32-bit integer safety

**File:** `dashboard/src/App.tsx:14`

**Issue:** The hash accumulation `h = (h * 31 + name.charCodeAt(i)) & 0xfffff` masks to 20 bits. This is functional but unusual — the `& 0xfffff` mask limits the hash space to 1,048,576 values with no documentation of why 20 bits was chosen over the conventional 32-bit mask. The low bit count increases gradient collision probability for projects with similar names.

**Fix:** Use `& 0xffffffff` for the conventional 32-bit hash, or document why a smaller mask is intentional.

### IN-03: `mbse-requirements.md` references `create_relationship` but `mbse-init.md` does not list it as an MCP tool

**File:** `packages/skills/skills/mbse-requirements.md:18,27`

**Issue:** The requirements skill calls `create_relationship("Dependency", reqId, snId)` but the init skill's tool list (`MCP Tools Used`) does not mention `create_relationship`. If this tool is not registered in the MCP server, the requirements skill will silently fail when attempting to trace to stakeholder needs. There is no fallback or error handling described.

**Fix:** Verify `create_relationship` is implemented in the MCP server and add it to the tool list in `mbse-init.md` if needed, or document that tracing is optional and handled gracefully when the tool is absent.

### IN-04: Unused `childrenOf` map in `buildRequirementsModel`

**File:** `dashboard/src/lib/diagram-generators.ts:251,284-285`

**Issue:** `childrenOf` is declared as a `Map<string, RequirementNode[]>` and populated in one branch of the ownership walk (`childrenOf.set('root', [...])` / `childrenOf.get('root')!.push(...)`), but it is never read. The actual roots are collected later by finding nodes not present in `childIds`. The `childrenOf` map is dead code.

**Fix:** Remove the `childrenOf` declaration and its two references (lines 251, 284-285).

---

_Reviewed: 2026-05-05_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
