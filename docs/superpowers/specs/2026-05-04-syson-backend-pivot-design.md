# SysON Backend Pivot — Design Spec

## Goal

Replace the split SMAPS + local JSON architecture with SysON as the single model store, gaining full SysML v2 containment hierarchy, metamodel-validated element creation, and native diagram rendering for all SysML v2 view types.

## Why

SMAPS returns flat element lists with empty `ownedElement`/`owner` fields and only supports PartDefinitions and Requirements. Local JSON files store proxy ports and connections without metamodel validation. SysON implements the full SysML v2 metamodel with proper containment, supports all element types (ports, actions, states, flows, interfaces, allocations, etc.), and enforces what children are valid under each container type via `childCreationDescriptions`.

## Architecture

**Before:** Express → SMAPS REST (`:9000`) for elements + local JSON for ports/connections. Mermaid for BDD, React Flow for IBD.

**After:** Express → SysON REST (`:8080/api/rest/`) for elements with full containment + SysON GraphQL (`:8080/api/graphql`) for mutations and tree metadata. SysON embedded diagrams for all view types. React Flow IBD as secondary viewer.

```
┌─────────────────────────────────────────────────────┐
│ Dashboard (React + Vite)                            │
│ ┌───────────┬──────────────────────┬──────────────┐  │
│ │ Sidebar   │ ProjectDetail        │ ChatPanel    │  │
│ │ (SysON    │ ┌──────────────────┐ │ (SSE stream) │  │
│ │  projects)│ │ ContainmentTree  │ │              │  │
│ │           │ │ (from SysON REST)│ │ Tools:       │  │
│ │           │ ├──────────────────┤ │ createChild  │  │
│ │           │ │ DiagramPanel     │ │ deleteItem   │  │
│ │           │ │ ┌──────────────┐ │ │ queryElements│  │
│ │           │ │ │SysON (embed) │ │ │              │  │
│ │           │ │ │IBD (RF, opt) │ │ │              │  │
│ │           │ │ └──────────────┘ │ │              │  │
│ │           │ └──────────────────┘ │              │  │
│ └───────────┴──────────────────────┴──────────────┘  │
└──────────────────────┬──────────────────────────────┘
                       │ /api/*
                ┌──────┴──────┐
                │ Express     │
                │ server.js   │
                └──────┬──────┘
          ┌────────────┼────────────┐
          │            │            │
   SysON REST    SysON GraphQL   Anthropic
   :8080/api/rest  :8080/api/graphql  API
```

## SysON API Surface Used

### REST API (`/api/rest/`)

Same SysML v2 standard as SMAPS. Key difference: containment fields (`ownedElement`, `ownedMember`, `owner`) are populated.

| Endpoint | Purpose |
|---|---|
| `GET /projects` | List all projects |
| `GET /projects/:id/commits` | Get HEAD commit ID |
| `GET /projects/:id/commits/:commitId/elements` | All elements with full containment |
| `POST /projects/:id/commits` | Create commit with element changes |

### GraphQL API (`/api/graphql`)

| Operation | Purpose |
|---|---|
| `viewer.projects(first, after)` | List projects (paginated) |
| `viewer.project(projectId)` | Get project + `currentEditingContext.id` |
| `editingContext.childCreationDescriptions(containerId)` | Valid child types for a container |
| `editingContext.object(objectId)` | Get single element |
| `editingContext.representations(...)` | List diagram representations |
| `editingContext.explorerDescriptions()` | Get explorer tree description ID |
| Mutation: `createProject(name, templateId)` | Create new SysMLv2 project |
| Mutation: `createChild(editingContextId, objectId, childCreationDescriptionId)` | Create element under parent |
| Mutation: `renameTreeItem(editingContextId, treeItemId, newLabel)` | Set element name |
| Mutation: `deleteTreeItem(editingContextId, treeItemId)` | Delete element |
| Mutation: `createRepresentation(...)` | Create a diagram view |

### Project Templates and Document Stereotypes

**Project templates** (for `createProject` mutation):

| Template ID | Label |
|---|---|
| `sysmlv2-template` | SysMLv2 (creates project with SysML v2 document pre-configured) |
| `blank-project` | Blank Project (empty, requires manual document creation) |

**Document stereotypes** (for `createDocument` mutation, to add a SysML document to an existing project):

| Stereotype ID | Label |
|---|---|
| `empty_sysmlv2` | SysMLv2 |
| `empty_sysmlv2_library` | SysMLv2-Library |

New project flow: `createProject(templateId: "sysmlv2-template")` → project is ready with a SysML v2 document. No separate `createDocument` step needed.

### Editing Context

Every SysON project has a `currentEditingContext` with an `id`. This ID is required for all GraphQL mutations. The Express server resolves it once per project and caches it.

## Components

### Sidebar (modified)

Fetches projects from SysON REST `GET /api/rest/projects` instead of SMAPS. "New Project" calls `createProject` GraphQL mutation with `templateId: "sysmlv2-template"`. Delete calls `deleteProject` mutation.

### ContainmentTree (new, replaces flat elements table)

Renders an expandable tree built from SysON's element data.

**Data fetching:** Single REST call to `GET /api/rest/projects/:id/commits/:commitId/elements` returns all elements with `ownedElement` arrays. Tree is reconstructed client-side.

**Membership filtering:** SysML v2's metamodel wraps children in `OwningMembership` and `FeatureMembership` nodes. The tree skips these — displays the owned element directly under its logical parent, matching SysON's explorer behavior. Specifically, for any element where `@type` ends in `Membership`, the tree renders its `ownedElement` children in place of the membership node itself.

**Tree node rendering:**
```
▼ 📦 Package: DroneSystem
  ▼ 🔧 PartDefinition: FlightController
      ⚡ PortUsage: pwr_in
      ⚡ PortUsage: ctrl_out
      🔗 ConnectionUsage: ctrlBus
  ▼ 🎬 ActionDefinition: ExecuteFlight
      ▶ ActionUsage: takeoff
      ▶ ActionUsage: navigate
      ◆ DecisionNode
  ▼ 📋 RequirementDefinition: MaxTakeoffWeight
      📝 Documentation
```

Each node shows: disclosure triangle (if `ownedElement.length > 0`), type icon, `@type` as muted label, element `name`.

**Element type icons:** Map `@type` to a small set of icons. Unknown types get a default icon. No need to cover every SysML v2 type — just the common ones (PartDefinition, PortUsage, ActionDefinition, RequirementDefinition, Package, ConnectionUsage, StateUsage, InterfaceDefinition, ItemDefinition, FlowUsage, etc.).

### DiagramPanel (simplified)

**Primary tab: SysON diagrams.** For each `representation` in the project (queried via `editingContext.representations`), show a tab. The diagram is displayed by embedding SysON's diagram view in an iframe pointed at `http://localhost:8080/projects/:projectId/edit/:representationId`. The chat assistant creates new diagram representations via `createRepresentation`.

**Secondary tab: IBD (React Flow).** Shown when the containment tree has `PortUsage` elements. Reuses existing `SysMLBlockNode`, `SysMLEdge`, and `ibd-layout.ts` components. The `ibd-transformer.ts` is updated to consume SysON element shapes (which have the same `@type`, `name`, `ownedElement`, `owner` fields — just with full containment populated).

**Removed:**
- Mermaid BDD auto-generation
- Stored Mermaid diagram tabs
- MermaidViewer component
- `mermaid` npm dependency

### ChatPanel (modified)

SSE streaming and conversation management unchanged. The tool suite in `server.js` changes:

**`query_elements` tool:** Queries SysON REST API instead of SMAPS. Returns elements with containment context (parent name, child count).

**`create_element` tool (replaces `create_local_element`):**
1. Resolve `editingContextId` for the project
2. Call `childCreationDescriptions(containerId)` to find the matching `childCreationDescriptionId` for the desired element type
3. Call `createChild` mutation (returns auto-named element)
4. Rename via REST commit API: `POST /api/rest/projects/:id/commits` with `DataVersion` change setting `declaredName`
5. Return the created element

**`delete_element` tool (replaces `delete_local_element`):** REST commit with `payload: null` to remove the element.

> **Note:** `renameTreeItem` and `deleteTreeItem` GraphQL mutations require a tree `representationId` that fails with a type-checking error in this SysON version. The REST commit API provides a reliable alternative for both rename and delete operations.

**`create_diagram` tool (replaces `render_diagram`):** Calls `createRepresentation` mutation to create a SysON diagram view. Returns the representation ID so the dashboard can display it.

### IBDViewer (modified)

Same React Flow canvas with `SysMLBlockNode` and `SysMLEdge` custom types. `ibd-transformer.ts` updated:

- Input: SysON elements (full containment, proper `owner` references with full UUIDs)
- `PartDefinition` → `SysMLBlockNode`
- `PortUsage` (child of a PartDefinition) → `Handle` on owning block
- `ConnectionUsage` → `SysMLEdge`
- No more prefix-matching hack for owner IDs — SysON returns full UUIDs
- `resolveBlockId` function removed (exact match only)

### App (modified)

- SMAPS status dot replaced with SysON status dot (health check against `:8080`)
- Project list from SysON REST
- `currentProject` uses SysON project shape (`{ id, name }` from GraphQL)
- `refreshKey` still triggers reload after chat mutations

## Express Server Changes

### Removed

- `SMAPS` constant and all SMAPS fetch calls
- `queryElements()` function (SMAPS query-results)
- `loadProjectData()` / `saveProjectData()` (local JSON)
- `/api/projects/:id/local-elements` endpoints
- `/api/projects/:id/diagrams` endpoints (stored Mermaid)
- `express.static(join(__dirname, 'data'))` if present
- All references to `dashboard/data/*.json`

### Added

- `SYSON_REST` constant: `process.env.SYSON_ENDPOINT || 'http://localhost:8080'`
- `SYSON_GQL` constant: `${SYSON_REST}/api/graphql`
- `sysonRest(path)` helper: fetches from SysON REST API
- `sysonGql(query, variables)` helper: executes GraphQL query/mutation
- `getEditingContextId(projectId)` helper: resolves and caches the editing context ID

### Updated Endpoints

| Route | Implementation |
|---|---|
| `GET /api/projects` | `sysonRest('/api/rest/projects')` |
| `GET /api/projects/:id/elements` | `sysonRest('/api/rest/projects/:id/commits/:commitId/elements')` |
| `DELETE /api/projects/:id` | `sysonGql(deleteProject, { projectId })` |
| `POST /api/projects` | `sysonGql(createProject, { name, templateId: 'sysmlv2-template' })` |

### Chat Tools

| Tool | Implementation |
|---|---|
| `query_elements` | Fetch from SysON REST, return with containment summary |
| `create_element` | `childCreationDescriptions` → `createChild` → `renameTreeItem` |
| `delete_element` | `deleteTreeItem` mutation |
| `create_diagram` | `createRepresentation` mutation |

## Files

### New

| File | Purpose |
|---|---|
| `src/components/ContainmentTree.tsx` | Expandable tree built from SysON elements |
| `src/lib/containment.ts` | Build tree from flat element array (skip memberships, resolve parents) |

### Modified

| File | Change |
|---|---|
| `server.js` | Replace SMAPS with SysON REST + GraphQL, update chat tools |
| `src/components/Sidebar.tsx` | Fetch from SysON, create/delete via GraphQL |
| `src/components/ProjectDetail.tsx` | Replace elements table with ContainmentTree |
| `src/components/DiagramPanel.tsx` | SysON iframe tabs + React Flow IBD tab, remove Mermaid |
| `src/components/IBDViewer.tsx` | Consume SysON element shapes |
| `src/lib/ibd-transformer.ts` | Remove prefix-matching, use SysON's full containment |
| `src/lib/api.ts` | Update endpoints, remove local-element and diagram CRUD |
| `src/components/App.tsx` | SysON health check, SysON project shape |
| `src/components/ChatPanel.tsx` | Updated tool names in status display |
| `src/types/sysml.ts` | Update types to match SysON element shape |

### Removed

| File | Reason |
|---|---|
| `dashboard/data/*.json` | Local element store deprecated |

### Unchanged

| File | Reason |
|---|---|
| `src/components/sysml/SysMLBlockNode.tsx` | Reused by React Flow IBD |
| `src/components/sysml/SysMLEdge.tsx` | Reused by React Flow IBD |
| `src/lib/ibd-layout.ts` | ELK layout unchanged |
| `src/index.css` | Dark theme, SysML node styles unchanged |
| `vite.config.ts` | Proxy target changes from `:6121` (stays same, Express proxies to SysON) |
| `packages/mcp-server/` | Claude Code MCP server unchanged |

## Dependencies

### Added

None. SysON APIs are called via `fetch`.

### Removed

| Package | Reason |
|---|---|
| `mermaid` | No longer rendering Mermaid diagrams |

## Error Handling

| Scenario | Behavior |
|---|---|
| SysON offline | Status dot red; sidebar shows "SysON unavailable"; tree and diagrams show offline message |
| Element creation fails | Chat reports the GraphQL error (e.g., invalid containment) |
| Diagram embed fails | iframe shows SysON's own error page; fallback message in tab |
| React Flow IBD fails | Same ELK fallback as current implementation |

## What Does NOT Change

- Express server as API gateway (same port 6121)
- Chat SSE streaming architecture
- React Flow IBD as secondary view (same components, just fed SysON data)
- Dark color theme and CSS tokens
- Three-column layout
- Conversation persistence (localStorage)
- `packages/mcp-server/` Claude Code MCP server
- Vite build tooling
- Docker compose stack (SMAPS stays available, SysON already running)
