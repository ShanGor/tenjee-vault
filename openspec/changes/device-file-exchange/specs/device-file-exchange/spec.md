# Spec Delta

## Purpose

Allow users to intentionally copy selected ordinary files and folder trees between devices over a reachable LAN or private VPN, with recipient consent, bounded resource use, verified output, and recoverable interruptions.

## ADDED Requirements

### Requirement: Separate vault and file exchange modes

Device Exchange SHALL offer explicit **Vault data** and **Files & folders** modes. Vault mode SHALL preserve existing bidirectional workspace exchange. File mode SHALL copy a sender-approved batch to one recipient without importing files into vault records, replicating source paths, deleting source content, or granting continuing peer trust. Each file session SHALL have exactly one sender and one recipient, independent of which device hosts the pairing listener. Incompatible modes or file protocol versions SHALL fail with an explanation before file payload transfer; the system SHALL NOT silently change modes or weaken authentication.

#### Scenario: Send files without exchanging vault data
- **WHEN** two devices select file mode and approve a file batch
- **THEN** only the selected batch is copied and neither workspace is replicated or modified by the transfer

#### Scenario: Old peer or incompatible mode
- **WHEN** the peer supports only vault exchange or both devices choose incompatible roles or modes
- **THEN** the system explains the required mode/version and transfers no file payload or vault records

### Requirement: User-selected files and complete folder trees

The sender SHALL be able to add one file, multiple files, and one or more folders to a batch through native selection, combine selections, remove items, and review counts and total bytes before offering the batch. Folder selection SHALL include regular descendant files, empty directories, and hidden entries, and SHALL preserve the selected root folder and nested relative structure. Overlapping selections SHALL NOT send the same selected entry twice. Symlinks, Windows reparse-point traversal, special files, inaccessible entries, and unrepresentable names SHALL be rejected or visibly listed as exclusions requiring sender acknowledgement before approval. Traversal SHALL remain within selected roots. Absolute local paths and native permission handles SHALL remain local.

#### Scenario: Mixed batch
- **WHEN** the sender adds three files and a folder containing nested files and empty directories
- **THEN** the preview includes all supported entries and the recipient receives their approved names and relative folder structure

#### Scenario: Folder includes unsupported or hidden entries
- **WHEN** a selected folder contains hidden regular files, a symlink, and an unreadable file
- **THEN** hidden regular files are included visibly, the unsupported entries are explained, and the offer cannot be approved until exclusions are acknowledged

#### Scenario: Overlapping selections
- **WHEN** the sender selects a folder and separately selects a file already included in that folder
- **THEN** the overlapping entry is included only once and the preview describes the resulting batch

### Requirement: Authenticated offer and recipient consent

Both devices SHALL explicitly enter exchange mode and authenticate using a fresh temporary eight-digit, five-minute, single-use code with a global five-failure limit, mutual key confirmation, encrypted transport, and session binding. File names, offers, resume metadata, and content SHALL NOT be disclosed before successful authentication. Both devices SHALL approve the same immutable offer, selected entries, naming plan, and transfer direction before content is sent. The recipient SHALL review peer information, filenames/tree, counts, total bytes, exclusions, and required storage, and select a writable destination before acceptance. Offer changes SHALL invalidate approval. Peer labels and discovery names SHALL NOT confer identity trust.

#### Scenario: Recipient declines
- **WHEN** pairing succeeds but the recipient declines the offered batch
- **THEN** no file content is transferred, existing files remain intact, and authorization ends

#### Scenario: Offer changes after preview
- **WHEN** a sender changes the selected batch after either side has approved its preview
- **THEN** the old approval cannot authorize the changed batch and both devices must review and approve the new offer

### Requirement: LAN and routed private VPN connectivity

File mode SHALL support automatic discovery on eligible LAN interfaces and manual hostname/IP with port on LAN and explicitly selected private VPN interfaces, including Tailscale IPv4 and private IPv6. Listeners and outbound connections SHALL use the selected local addresses and validate eligible peer addresses. Tailscale MagicDNS names SHALL use the OS resolver and the same address validation and temporary pairing as numeric addresses. A locally saved fixed port SHALL be optional; an occupied fixed port SHALL fail without substitution. Discovery failure SHALL NOT prevent a manual connection to a reachable peer. DNS and connection attempts SHALL be bounded and cancellable. Public Internet endpoints, Internet discovery, automatic router port mapping, and an app-operated relay SHALL remain unsupported; VPN-provided routing or relaying SHALL be allowed.

#### Scenario: LAN multicast blocked
- **WHEN** both devices are reachable on LAN but local discovery is blocked
- **THEN** entering the recipient's hostname/IP and listening port permits the same authenticated file transfer

#### Scenario: Tailscale between different networks
- **WHEN** the selected Tailscale interfaces and VPN access rules permit peer traffic and the sender enters the receiver's MagicDNS name or overlay IP and port
- **THEN** file transfer works without shared LAN discovery, including when the VPN transports traffic through its own relay

#### Scenario: Routed VPN or IPv6
- **WHEN** the user selects a VPN interface and an eligible routed private IPv4 or IPv6 peer outside the interface's local subnet is reachable
- **THEN** file mode connects through that interface and retains the same authentication and approval requirements

#### Scenario: Peer blocked by VPN access rules
- **WHEN** DNS resolves but the peer's exchange port is unreachable
- **THEN** the app stops within a bounded connection budget and explains checks for the peer's mode, port, firewall, and VPN access rules

### Requirement: Streaming and bounded transfer resources

File mode SHALL stream payloads without holding entire files or batches in memory, encoding file bodies as text, or requiring a whole-batch archive or staging copy before sending. It SHALL support files above 4 GiB and batches above 1 GiB when source and destination storage support them. Protocol fields SHALL represent lengths without truncation; platform limits SHALL be reported before affected output is published. Manifest size, entry count, path depth, frame length, queued data, and resume metadata SHALL have explicit validated limits independent of vault attachment limits. Insufficient storage or unavailable source access SHALL produce an actionable partial/failure result without damaging existing destination data. Preparation and transfer SHALL remain responsive and cancellable.

#### Scenario: Large file
- **WHEN** the sender selects an 8 GiB regular file and the recipient has sufficient compatible storage
- **THEN** the file transfers with bounded memory and is not rejected by vault attachment or mobile import-cache ceilings

#### Scenario: Disk becomes full during transfer
- **WHEN** free space is exhausted after approval
- **THEN** transfer stops with an insufficient-space explanation, completed output remains valid, and uncompleted files remain visibly incomplete and eligible for recovery

#### Scenario: Malicious oversized declaration
- **WHEN** a peer declares an excessive frame, manifest, entry count, depth, or overflowing total
- **THEN** the declaration is rejected before excessive allocation or destination writes

### Requirement: Verified and safe destination output

The recipient SHALL validate relative paths and file lengths, verify received bytes against sender-provided integrity values, and publish each completed file only after verification and required durable local writes. Normal transfers SHALL create a new uniquely named batch directory below the selected destination and SHALL NOT overwrite or merge into existing user files. Conflicts within the batch or names incompatible with the destination SHALL have a deterministic keep-both mapping or explicit rejection shown before approval. Traversal, absolute paths, alternate data streams, and symlink/reparse escapes SHALL be rejected, including races during output creation. Unknown executable content SHALL NOT be launched automatically. Empty files and empty directories SHALL be preserved. Unsupported provider publication semantics SHALL be explained before acceptance and SHALL NOT produce a false durability or completion claim.

#### Scenario: Verify and publish a file
- **WHEN** all bytes arrive with the approved length and matching integrity values
- **THEN** the file is published under its approved destination name and appears as completed only after output finalization succeeds

#### Scenario: Hash mismatch or path escape
- **WHEN** bytes fail integrity verification or a path would write outside the selected destination
- **THEN** no affected file is published as complete and the app reports the rejection

#### Scenario: Duplicate roots and existing output
- **WHEN** two selected roots share a basename and the destination already contains earlier received files
- **THEN** the preview shows distinct names for the roots, transfer uses a new batch directory, and the earlier files remain unchanged

### Requirement: Durable resume and honest partial results

Interrupted transfers SHALL retain durably verified progress and completed-file receipts independently of the live pairing session. Resume SHALL require explicit user action, fresh pairing, and mutual approval of the same batch and retained destination. Resume identity SHALL be bound to the retained sender batch and approved offer; filenames, lengths, device labels, or an untrusted offset alone SHALL NOT authorize reuse. Retained bytes SHALL be revalidated against local integrity records and the current source prefix before they are skipped. Changed, missing, corrupted, or inaccessible source/partial data SHALL cause the affected file to restart with suitable approval or fail explicitly, never yield a mixed file reported as complete. Retried delivery SHALL NOT duplicate already verified completed files. The result SHALL distinguish completed, excluded/skipped, failed, incomplete/resumable, and unconfirmed outputs; success SHALL require recipient completion acknowledgement for every accepted entry.

#### Scenario: Disconnect during large file
- **WHEN** Wi-Fi or VPN connectivity fails midway through a file
- **THEN** a later freshly paired and approved session reuses verified durable progress, retransmits any uncheckpointed tail, and preserves already completed files without duplicates

#### Scenario: Modified partial or source
- **WHEN** retained partial bytes are corrupted or the source prefix differs before resume
- **THEN** invalid bytes are not reused and the affected file restarts with approval or is reported as changed/failed

#### Scenario: Completion acknowledgement lost
- **WHEN** the recipient publishes output but the connection drops before the sender receives its completion receipt
- **THEN** the sender reports an unconfirmed result and a newly authorized resume reconciles the receipt without creating another copy

### Requirement: Explicit lifecycle, progress, and cleanup

Preparation, transfer, verification, and finalization SHALL display their current phase. During transfer the app SHALL show completed file count, transferred versus total bytes, current entry, measured speed, and an ETA when enough evidence exists. Displayed durable progress SHALL be distinguishable from in-flight bytes. Stop or Pause SHALL end network authorization while preserving committed files and verified resumable progress; completion, app suspension/closure, or 30 minutes without meaningful user/transfer activity SHALL also end authorization. Prior history SHALL NOT reconnect or keep listeners active. Users SHALL be able to resume or discard an incomplete batch; discard SHALL remove only app-owned partial data and associated recovery metadata, never source files or completed output. Cleanup SHALL NOT silently remove a pending resume within seven days of its last activity, and low-storage cleanup SHALL require explicit discard.

#### Scenario: Stop then resume
- **WHEN** either device stops an active batch and later chooses Resume
- **THEN** listeners and sockets have closed, completed files are retained, and the next session requires fresh code authentication and approval

#### Scenario: Verification phase and unavailable ETA
- **WHEN** all bytes have arrived but verification/publication is still running or speed is not yet stable
- **THEN** the UI shows the actual phase and does not announce completion or invent an ETA

#### Scenario: Discard pending batch
- **WHEN** the user discards an incomplete batch
- **THEN** only its owned partial files and recovery records are removed, completed output and selected sources remain intact, and no network permission survives

### Requirement: Native platform file access

Desktop SHALL support native multiple-file and folder selection and native destination selection. Android SHALL support multiple document selection and source/destination document-tree selection within platform grants, without requiring broad storage permission or resolving provider URIs into fabricated filesystem paths. Native handles and resumable permission grants SHALL remain local. Providers lacking reliable size, seek, or publication operations SHALL expose the required bounded staging/restart behavior and storage cost before approval, or be rejected with guidance to choose a compatible local destination. Selection dialogs SHALL be scheduled before network authorization so their platform lifecycle transitions do not silently invalidate an approved session. iOS file mode SHALL be unavailable with an explanation until its native adapter is implemented.

#### Scenario: Android folder transfer
- **WHEN** an Android user selects a permitted document tree and the peer accepts its contents
- **THEN** supported descendants and empty directories transfer through native access with the approved relative structure and no broad storage permission

#### Scenario: Non-seekable Android provider
- **WHEN** a provider cannot seek or supply a reliable size
- **THEN** preparation explains a bounded per-file staging requirement or unsupported source, and resume does not claim arbitrary offset access

#### Scenario: System picker pauses Android activity
- **WHEN** the user chooses source files or a receive directory using the system picker
- **THEN** selection finishes before entering exchange mode and a picker pause does not masquerade as an unexplained transfer failure

### Requirement: Measurable speed and recovery acceptance

Release acceptance SHALL include reproducible measurements with a 4 GiB incompressible file, an 8 GiB boundary test, and a 10,000-file tree with nested/empty directories. Tests SHALL compare large-file payload throughput against a minimal streaming transfer using the same encryption, devices, route, storage, and durability policy; the target SHALL be at least 70 percent of that baseline on controlled LAN and routed-VPN profiles through 80 ms RTT. Reports SHALL separately state preparation, payload, verification, publication, peak memory, and durable resume/retransmission results. Validation SHALL cover actual LAN, Tailscale, and another routed VPN, interrupted transfers, app/process restart, permission loss, storage exhaustion, naming collisions, malformed input, and unchanged vault exchange. Tailscale direct and relayed results SHALL be recorded separately when those routes are available; no result SHALL imply a fixed absolute VPN speed.

#### Scenario: Reproducible performance report
- **WHEN** the implementation is evaluated on the controlled test profiles
- **THEN** the report records the baseline, relative throughput, all transfer phases, memory, and route conditions and identifies any acceptance target that was not met

#### Scenario: Recovery acceptance
- **WHEN** test runs disconnect and crash at checkpoint, verification, publication, and receipt boundaries
- **THEN** recovered output remains correct, existing files remain unchanged, only verified durable progress is reused, and sender results match confirmed or unconfirmed recipient outcomes
