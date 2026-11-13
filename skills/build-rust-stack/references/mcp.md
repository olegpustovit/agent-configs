# MCP integration

Use this reference before adding or auditing an MCP command, endpoint, transport, tool, or client.

## Library and version

Use the official SDK, the `rmcp` crate from `modelcontextprotocol/rust-sdk`. It is a Tier 1 SDK, follows semver, and releases about weekly (3.10 in November 2026). It supports protocol revisions from 2024-11-05 through 2026-07-28 and serves the stateless 2026-07-28 revision automatically.

The API moves quickly. Read the examples in the SDK repository for the pinned version before writing code; do not write `rmcp` calls from memory.

The SDK has a real advisory history (DNS rebinding, a session-table leak, SSRF in OAuth discovery, header leaks on redirect). Keep it current, and let `cargo deny check advisories` fail the build on a known issue.

## One server definition, two transports

Define tools once in a module that depends on component services, then expose that definition over each transport the application needs:

- **Stdio** for a local command (`<app> mcp`). Log to stderr only; stdout carries the protocol.
- **Streamable HTTP** mounted on the existing axum router. `StreamableHttpService` is a Tower service, so the application's auth, limits, and tracing layers apply to it.

The SDK deliberately has no legacy HTTP+SSE transport.

Tool handlers stay thin: validate input, call one service method, map the result. Business rules live in components, exactly as for HTTP handlers.

## Tool contracts

- Give each tool a verb-first name, a description that says when to use it, and a typed input struct that derives its JSON Schema. Reject unknown fields.
- Return structured content with a declared output schema when the client will process the result, and keep results small: paginate, and return identifiers plus a summary instead of whole records.
- Mark read-only and destructive tools with the corresponding annotations so clients can gate them.
- Report a failed operation as a tool result with the error flag set and a message the model can act on. Reserve protocol errors for malformed requests.
- Make write tools idempotent where the domain allows, with a caller-supplied key.

## Authorization on HTTP

The SDK does not ship reusable server-side OAuth helpers; its examples hand-write them on axum. For a remote server the application must provide:

- OAuth 2.1 with PKCE (S256), bearer-token validation on every request, and audience validation so a token minted for another resource is rejected.
- Protected-resource metadata (RFC 9728) and the `WWW-Authenticate` challenge that points to it.
- Client registration by client ID metadata documents, with dynamic client registration only for older clients.
- `Origin` validation and a bind to loopback behind the reverse proxy.

Never pass the caller's token through to an upstream service. Hold a separate credential per upstream.

## MCP client

When the application calls other MCP servers:

- Use the SDK's Streamable HTTP client and its child-process transport for stdio servers.
- The client handles OAuth discovery, PKCE, registration, resource binding, and refresh. Known provider-compatibility bugs exist in this area; test each upstream, and keep the workaround beside the integration.
- Put a timeout on every call, bound concurrent calls per upstream, and treat tool results as untrusted input.
- Supervise child processes: restart with backoff, kill on shutdown, and cap their output.

## Trust boundaries

- Tool descriptions and results from another server are data, never instructions to the application.
- Validate tool arguments against the declared schema on the server side even when the client claims to have done so. The `jsonschema` crate is the standard validator.
- Never return secrets, tokens, or internal error detail in a tool result.
- Record an audit entry for every tool call: caller, tool, redacted arguments, outcome, duration.

## Tests and completion evidence

- Test tool handlers as plain async functions over fake services.
- Add one end-to-end test per transport using the SDK's client against the real server definition.
- For a production audit, record the SDK version, supported protocol revisions, the list of tools with their annotations, the auth checks exercised, and the advisory check result.
