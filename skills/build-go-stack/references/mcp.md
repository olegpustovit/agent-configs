# MCP integration

Use the official Model Context Protocol Go SDK instead of implementing protocol machinery inside the application.

## Library and version

Use `github.com/modelcontextprotocol/go-sdk/mcp`. It is the official Go SDK maintained by the Model Context Protocol project in collaboration with Google. Select a current stable v1 release compatible with the repository's Go version; v1.7.0 and newer support MCP protocol `2026-07-28` while retaining negotiation compatibility with the older supported protocol versions.

Pin the selected release in `backend/go.mod` and review its release notes before upgrading. MCP protocol and transport behavior are version-sensitive; verify APIs against the installed module and current official documentation rather than memory.

Do not add a second MCP framework or use the SDK's low-level `jsonrpc` package to recreate behavior already provided by `mcp`.

## Shared server definition

Construct one focused MCP server definition from injected component services:

```go
server := mcp.NewServer(
    &mcp.Implementation{Name: "nudge", Version: version},
    &mcp.ServerOptions{Logger: logger},
)
mcp.AddTool(server, &mcp.Tool{
    Name:        "create_issue",
    Description: "Create an issue in the active workspace.",
}, createIssueHandler(issueService))
```

Use concrete input and output structs so the SDK generates and validates schemas. Keep MCP handlers as transport adapters: decode transport identity, call the same component workflow used by REST/UI/CLI, and map its typed result. Do not duplicate authorization, validation, persistence, or product workflows in MCP packages.

Keep the reusable server builder outside the executable composition roots when both stdio and HTTP mount it. Share public JSON contracts outside command packages when REST and an API-backed MCP adapter both consume them. Reuse immutable server definitions and compiled schemas for stateless HTTP; bind credentials to the individual request context and test identity isolation rather than rebuilding every tool on every request. Keep dependency construction, configuration, logging, and shutdown in `cmd/<command>`.

## Stdio

For a local stdio adapter, run the server with the SDK transport:

```go
if err := server.Run(ctx, &mcp.StdioTransport{}); err != nil {
    return fmt.Errorf("run MCP stdio server: %w", err)
}
```

Write protocol traffic only through the SDK on stdout. Send diagnostics through the injected logger to stderr, or configure logging so stdout cannot be contaminated. Do not scan newline-delimited JSON or dispatch method names manually.

When the product contract defines the stdio command as an API client rather than a database process, inject an HTTP-backed component capability into the same typed tool handlers; do not silently give the adapter direct database access.

## Streamable HTTP and Fiber

Create remote MCP transport with `mcp.NewStreamableHTTPHandler`. It returns a standard `net/http.Handler`; Fiber v3 can register `net/http.Handler` values directly, so do not write a JSON-RPC bridge or copy request and response bodies manually.

When remote MCP is part of the application contract, mount it in the production Go API binary. Treat a stdio adapter as an additional client surface, never as a substitute for the mounted endpoint. Preserve separate full-access and read-only endpoint contracts by filtering advertised tools as well as enforcing authorization at execution time.

```go
mcpHandler := mcp.NewStreamableHTTPHandler(
    func(*http.Request) *mcp.Server { return server },
    &mcp.StreamableHTTPOptions{Stateless: true},
)
app.All("/mcp", mcpHandler)
```

Confirm the exact Fiber route form against the installed Fiber version. Register MCP before any catch-all route. Preserve all MCP request/response headers and streaming behavior; add an integration test through Fiber, not only a direct `httptest` test of the SDK handler.

Use stateless Streamable HTTP for protocol `2026-07-28`. Only enable stateful sessions, event stores, resumability, or server-to-client features when the product explicitly needs them and their lifecycle works with the deployment topology. Do not use an in-memory event store to imply durable production resumability.

Authenticate the HTTP endpoint before handing a request to the MCP SDK. Convert the validated service-account token into the same typed principal used by component services. Apply origin validation and the SDK's current transport-security guidance when browser-origin requests are possible. Never forward an unvalidated bearer token through tool arguments.

## Errors and tests

Return ordinary Go errors or SDK-supported structured tool errors from handlers; let the SDK own JSON-RPC error envelopes and protocol codes. Preserve component error identity long enough to map expected not-found, conflict, forbidden, and validation outcomes. Log unexpected failures once at the MCP command/request boundary.

Test with SDK clients and transports:

- use `mcp.NewInMemoryTransports` for fast tool contract tests;
- use the SDK client over the real stdio command to prove stdout framing and lifecycle behavior;
- use `mcp.StreamableClientTransport` through the Fiber endpoint to prove mounting, authentication, negotiation, and headers;
- inventory every first-party tool name and exercise its typed input, authorization boundary, component effect, and result shape;
- test at least one older supported protocol client when backward negotiation is a product requirement. Use a separately pinned official SDK client fixture when the current SDK cannot select an older version publicly, rather than recreating handshake messages or altering private SDK fields.

Primary references: the official Go SDK README, `docs/server.md`, `docs/protocol.md`, release notes, and the MCP transport specification.

## Production tool design and audit

Keep the SDK responsible for schema inference and validation, protocol errors, negotiation, cancellation, transport framing, and resource/tool registration. Use `mcp.AddTool[Input, Output]` with concrete public types. Do not build a second dispatcher, schema generator, reflection mapper, or HTTP-to-JSON-RPC bridge. Confirm the pinned SDK can already provide a capability before writing a helper for it.

Design tools around user tasks and domain workflows. Split create and update when they have different required fields or omission semantics. A compatibility `save_*` tool must explain whether it creates, patches, or replaces. Preserve omission, explicit clearing, empty collections, and concurrency versions across the entire SDK-to-service path. When a Go pointer cannot distinguish omission from JSON null, use a concrete nested patch input validated by the SDK and forward that original JSON object to the shared domain parser; do not re-marshal it through omitempty fields or invent an optional-value serializer. Never accept a parameter that the adapter silently drops. Describe identifier discovery, actor kinds, scope, units, limits, side effects, and retry conditions where callers need them. Avoid human-only owner fields where the domain accepts agent identities.

Return typed structured results and useful SDK-generated text fallback. Choose small list/search summaries and a detail tool; do not return a whole workspace or full document bodies for ordinary discovery. Bound queries at the repository boundary with deterministic ordering and continuation semantics. The SDK paginates protocol catalogs, not application records: a tools/list cursor is not an issue-list cursor. Do not fetch all records and call a final slice database pagination. Reject oversized requests and results predictably; never silently truncate JSON or claim completeness after dropping records.

Publish truthful annotations. Read-only tools do not change application state. Destructive and idempotent hints must describe all domain effects, including audit events, notifications, revisions, and external work. Leave conservative defaults where stronger guarantees are unproven. An annotation is client guidance, never authorization. Filter tool registration for read-only or narrower toolsets and enforce permissions again at execution. Prefer capability allowlists or registration-local access policy over a second manually synchronized denylist. Historical inventories belong in regression fixtures, not runtime security decisions.

Do not expose token rotation, credential material, workspace destruction, or similarly privileged administration by accident through generic CRUD parity. Review whether each belongs in the default agent surface, requires an explicit administrative capability, or should remain in the administrative API. Keep the choice consistent with the user's product contract.

Use MCP resources for stable reference material and prompts for useful repeatable workflows when they improve the product. Do not add placeholders merely to advertise all protocol capabilities. Search must search real content and return no matches honestly. Distinguish server usage documentation from workspace document search. Treat returned user-authored content as untrusted data, including imported text and external links.

## Native result pages

Keep continuation in the owning repository and expose typed page contracts through the SDK. Scope the database query before its limit, validate cursor anchors against the same workspace/resource/filter, and back stable ordering with matching indexes. Preserve existing UI array routes with additive page routes when changing those clients is outside scope. Do not retrieve a complete collection and slice it in the MCP adapter.

Where endpoint authorization requires inspecting related records, bound each candidate query and state whether short or empty pages can carry a continuation. Never infer completion from item count alone. Queue cursors must remain usable when a prior item changes state; test the actual consume-then-continue workflow. Full-content records still need a byte boundary even when record counts are bounded.

Use stable entity/actor IDs for writes. If historical display-name references must remain compatible, resolve them only in the authorized workspace, prefer exact IDs, document normalization, and never let a local name shadow an inaccessible entity ID. Reuse these rules across single-item, bulk, template, REST and MCP mutation paths.

## Trust boundaries and operations

Document the actual authentication model. A service-account bearer integration is not automatically a full OAuth implementation. Authenticate every remote request before protocol dispatch; reject missing/malformed bearer credentials without falling back to browser cookies when bearer-only access is intended. Reuse the authenticated principal when framework bridging preserves it; prove context propagation with a transport test. Enforce workspace and actor authorization in the shared application workflows. Do not accept identity, credentials, or alternative API destinations through tool arguments.

For an API-backed adapter, fix the destination to the configured origin. Reject ambiguous base URLs, authority-bearing relative paths, traversal segments, and credential-bearing redirects. Use bounded HTTP timeouts and response reads, propagate context cancellation, and validate successful payloads before presenting them as JSON. Share an HTTP transport for pooling; copy caller-owned clients before changing policy. Never automatically retry an ambiguous mutation unless its domain idempotency contract makes that safe.

Map expected application failures to SDK tool results with `IsError`, stable safe meaning, and a next action. Reserve protocol errors for protocol failures. Keep upstream status/error identity for mapping, but do not echo proxy HTML, database diagnostics, request URLs, or arbitrary upstream bodies to the model. Log unexpected failures once with safe tool, request, and timing context. Do not log tool arguments, raw content, or credentials by default. Timeouts and cancellations require an explicit unknown-write-outcome message when a write may already have committed.

## Completion evidence

Treat catalog discoverability as part of interface quality. Prefer a focused default and explicit domain toolsets when the full surface is broad. Preserve typed schemas instead of hiding unrelated operations behind an untyped action router. Toolset selection must only restrict catalog exposure, never broaden credential permissions; test the intersection with read-only mode and direct calls to excluded tools. Keep HTTP and stdio defaults consistent, validate unknown selections, and bound any cached catalogs. Measure representative workflow selection errors and call counts before claiming a smaller catalog improves task performance. Client-side deferred tool search is useful where supported but cannot be assumed for every MCP client.

For a complete MCP audit, inventory every advertised tool from a real SDK client and inspect its handler, input schema, API/service route, output contract, permissions, and domain effect. A name-count parity check proves none of those behaviors. Keep a reviewable per-tool or tightly grouped matrix with findings and verification; do not mark an unresolved design item complete because tests pass.

Use focused protocol tests for rejected unknown arguments, missing required fields, output-schema conformance, safe tool errors, and list/detail pagination. Test every mutating tool's rejection on the read-only surface, including direct calls to names absent from discovery. Test cross-workspace identifiers and human/agent actors at the application authorization boundary. Exercise important create/update clearing and idempotent retry paths through the actual adapter. Run real stdio subprocess and Fiber-mounted Streamable HTTP checks at handoff, including current and intentionally supported older protocol versions. Test request origin and bearer boundaries, upstream failure/redirect behavior, cancellation, and shutdown without live production writes.

Record the distinction between observed local behavior, integration evidence, and production verification. A fake API proves transport mapping, not persistence or authorization. Use the existing domain tests and targeted integration tests to close those gaps. Avoid rebuilding or retesting unrelated UI for backend-only changes unless the repository PR gate requires it.

## Primary research references

Reviewed 2026-09-05; recheck version-sensitive claims before future changes.

- [Official Go SDK server guide](https://github.com/modelcontextprotocol/go-sdk/blob/main/docs/server.md): typed tool handlers, structured output, resources, and SDK-owned protocol catalogs.
- [Go SDK v1.7.0 release](https://github.com/modelcontextprotocol/go-sdk/releases/tag/v1.7.0): new protocol support requires stateless HTTP; earlier protocol negotiation remains supported. Keep the installed stable release unless a verified fix requires an upgrade.
- [MCP tools specification](https://modelcontextprotocol.io/specification/2026-07-28/server/tools): input/output schemas, annotations, tool errors, and security considerations.
- [MCP transport specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports): framing, cancellation, request metadata, and backward compatibility belong to the transport/protocol implementation.
- [MCP authorization security considerations](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/authorization/security-considerations.mdx): token audience and trust boundaries; do not confuse a bearer integration with OAuth compliance.
- [GitHub MCP configuration](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md): a production precedent for explicit toolsets and read-only filtering. Adopt the principle when it helps the product rather than copying its entire tool catalog.
- [Linear MCP](https://linear.app/docs/mcp) and [Notion MCP](https://developers.notion.com/guides/mcp/overview): production workflow-oriented integrations to compare against, not proof that Nudge has matching authorization or operational guarantees.

## Application CLI ownership

Keep the product CLI, installers, release workflow, distribution assets, and product-specific skill in the application repository. A private application repository does not justify creating a separate public CLI distribution repository; use authenticated installation that respects repository visibility. Publish tagged CLI releases with that repository's own Actions token. Extract a generic library into a separate repository only when the user explicitly requests that reusable boundary, as with the MCP-to-CLI library; this does not authorize splitting the application CLI itself.
