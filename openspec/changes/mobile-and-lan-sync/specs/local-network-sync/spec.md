# Spec Delta

## Purpose

Allow a user's own devices on a reachable LAN or private VPN, including Tailscale, to exchange local workspace changes securely through an explicit, temporary pairing session without an account or hosted sync service.

## ADDED Requirements

### Requirement: Explicit local discovery

The system SHALL expose “Find other devices and exchange” on desktop and mobile. Discovery, advertising, and incoming sync connections SHALL be disabled outside this mode. Both devices SHALL enter the mode, and discovered entries SHALL show a device label and platform, not workspace content. Automatic selection SHALL remain limited to directly connected private LAN peers. Users SHALL be able to select an eligible local interface explicitly for routed private/VPN peers, including Tailscale's 100.64.0.0/10 addresses and private IPv6 addresses. Listeners and outbound source addresses SHALL use the selected network addresses. Incoming peers SHALL be validated against the receiving listener's interface. Discovery/advertising SHALL be restricted to LAN interfaces. The app SHALL NOT operate a cloud relay, router port mapping, or Internet discovery; a user-managed VPN may supply its own routing/relay. Network/permission failures SHALL offer actionable guidance without disabling offline features.

#### Scenario: Discover a laptop and phone
- **WHEN** both devices enter exchange mode on a local network that permits peer traffic
- **THEN** the user can select the other device and begin pairing

#### Scenario: Router blocks peer discovery
- **WHEN** no peers are discovered because multicast or guest-network isolation blocks traffic
- **THEN** the app offers manual address entry and explains that both devices need a mutually reachable network

### Requirement: Hostname and configurable-port connections

Manual entry SHALL accept a hostname or numeric IP plus a nonzero port, including bracketed IPv6 with a numeric link-local scope. The system SHALL resolve hostnames with the OS resolver, validate each numeric result against the session's network policy, and attempt eligible results without resolving again. Public, loopback, unspecified, multicast, IPv4-mapped IPv6, unscoped/wrong-scope link-local, and zero-port targets SHALL be rejected. DNS waiting and TCP connection attempts SHALL be bounded and cancellation SHALL prevent a late DNS response from starting a connection. The receiver SHALL support automatic ports or an optional locally saved fixed port from 1–65535. An occupied fixed port SHALL produce an error without silent port substitution. Hostnames SHALL NOT replace temporary code authentication or mutual approval.

#### Scenario: Connect using Tailscale MagicDNS across networks
- **WHEN** both devices select their Tailscale interface, their VPN permits peer traffic, and the user enters the receiver's resolvable MagicDNS name and listening port
- **THEN** the app connects using eligible resolved overlay addresses, authenticates with a fresh code, and requires approval on both devices before transferring workspace payloads

#### Scenario: Use a regular VPN with routed peers
- **WHEN** both devices select their VPN interface and the peer has a reachable private address outside the local interface's subnet
- **THEN** manual hostname/IP and port entry can connect without requiring a directly connected subnet

#### Scenario: Resolve multiple addresses
- **WHEN** a hostname returns public and eligible private addresses or an unreachable eligible address before a reachable one
- **THEN** only eligible numeric addresses are attempted, IPv4/IPv6 alternatives are tried within a bounded budget, and public addresses are never connected

#### Scenario: Stop during hostname lookup
- **WHEN** the user stops exchange before DNS returns
- **THEN** the session ends without waiting indefinitely for the system resolver and a late DNS result cannot open a connection

#### Scenario: Reuse a fixed port
- **WHEN** the user configures a fixed listening port and starts a later exchange
- **THEN** listeners reuse that port while active, and an unavailable port prompts the user to choose another port or automatic allocation

### Requirement: Temporary code authentication

The receiving device SHALL display a cryptographically random eight-digit single-use authentication code valid for five minutes. The connecting device SHALL require that code. The pairing protocol SHALL resist passive offline guessing, authenticate the peers and negotiated session, and require mutual key confirmation. Five failed authentication attempts across all connections for the code SHALL invalidate it. Codes SHALL NOT be advertised, transmitted as plaintext, or logged. Successful authentication SHALL consume the code and admit only one peer to the session. A new session SHALL require a new code.

#### Scenario: Pair successfully
- **WHEN** the user enters the receiving device's valid code on the connecting device
- **THEN** both devices authenticate the same session and proceed to exchange approval without transferring workspace data yet

#### Scenario: Expired, reused, or guessed code
- **WHEN** a code expires, is reused, or accumulates five failed attempts
- **THEN** pairing fails, no workspace content is exchanged, and the user must generate a new code

### Requirement: Mutual approval and bounded encrypted sessions

After authentication, each device SHALL display the peer identity, full-workspace exchange scope, and a change/conflict summary. Workspace payloads SHALL transfer only after both users approve that scope. Traffic SHALL have confidentiality, integrity, replay protection, and authentication bound to the pairing session. Session keys SHALL remain in memory and be cleared on termination. The user SHALL be able to stop exchange at any time; completion, cancellation, app suspension/closure, or 30 minutes without transfer/user activity SHALL close the listener and terminate authorization. Prior sync history SHALL NOT authorize another session.

#### Scenario: Decline exchange approval
- **WHEN** either device declines approval after authenticating
- **THEN** neither device transfers workspace payloads and the session closes

#### Scenario: Finish an exchange
- **WHEN** both devices acknowledge all approved changes and validated attachments
- **THEN** the app displays the result and ends the network session, requiring fresh pairing for the next exchange

### Requirement: Bidirectional workspace replication

The system SHALL exchange spaces and page trees, page content/history, applicable templates, protection metadata, tasks/lists including medication doses and linked checkboxes, calendar events/exceptions/reminders, tags/associations, and attachments. It SHALL preserve logical identifiers, hierarchy, and cross-module links, transfer only missing changes/blobs after initial synchronization, and merge independently created workspaces without replacing either workspace. It SHALL exclude device paths, appearance/navigation preferences, permissions, notification delivery history, local pending work queues, backup directories, and unlocked sessions. Replication SHALL NOT use whole-vault restore or live database file replacement.

#### Scenario: Initial exchange into an empty phone
- **WHEN** the laptop has content and the paired phone has an initialized but otherwise empty workspace
- **THEN** the phone receives the laptop's content and links while retaining its own settings and both devices retain stable logical resource identifiers

#### Scenario: Exchange offline changes in both directions
- **WHEN** both previously synchronized devices create different objects while offline and later exchange
- **THEN** both devices receive the other device's objects and converge without duplicates on repeated exchange

### Requirement: Visible concurrent-change handling

The system SHALL distinguish causal successors from concurrent revisions without using wall-clock timestamps as the conflict authority. It SHALL apply nonconflicting changes automatically and retain concurrent edits, edit-versus-delete changes, conflicting moves, and protection changes as durable conflicts. It SHALL NOT silently discard a variant. Conflict controls SHALL let users select a variant or keep both, creating a resolution that acknowledges all competing revisions. Structural cycles and invalid protection nesting SHALL be rejected and retained for resolution. Protected conflict contents SHALL remain locked until the appropriate password is supplied.

#### Scenario: Both devices edit the same note offline
- **WHEN** the phone and laptop change the same page from the same synchronized revision
- **THEN** exchange preserves both variants, shows an unresolved conflict on both devices, and propagates a later resolution

#### Scenario: Delete conflicts with an offline edit
- **WHEN** one device deletes a task and another concurrently edits it
- **THEN** neither action silently wins and the app retains the deletion and edited variant for explicit resolution

### Requirement: Recoverable transfer and compatibility

The system SHALL validate protocol and schema compatibility before workspace writes, enforce bounded payload and attachment limits, verify attachment integrity before making references visible, and apply changes idempotently. Interruptions SHALL retain valid committed changes and resumable progress, never report a partial exchange as complete, and require fresh pairing after termination. Unsupported revisions, malformed data, insufficient storage, or invalid hierarchies SHALL be reported without destroying existing local data. A final summary SHALL distinguish applied changes, skipped/rejected changes, unresolved conflicts, and pending attachments.

#### Scenario: Wi-Fi disconnects during attachment transfer
- **WHEN** a connection fails before an attachment is completely received
- **THEN** no visible record points to a partial attachment and a newly paired exchange resumes from verified progress without duplicating committed objects

#### Scenario: Incompatible application versions
- **WHEN** peers have incompatible sync protocol or entity schemas
- **THEN** exchange stops before writing workspace data and both devices receive an upgrade explanation

Legacy protected domains with unmigrated plaintext titles SHALL remain pending until successful local unlock migrates those titles; inventory SHALL NOT disclose their title payloads.
