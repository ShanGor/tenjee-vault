# Spec Delta

## ADDED Requirements

### Requirement: Ciphertext-preserving protected-tree replication

The system SHALL synchronize protected page trees, versions, templates, and attachment blobs as their existing ciphertext together with wrapped DSK, salt, KDF parameters, verifier, and required domain references. It SHALL NOT transfer passwords, unwrapped DSK/KEK, decrypted titles/content, or in-memory search indexes, even when the source is unlocked. Pairing SHALL NOT grant unlock authority; received protected content SHALL remain locked until locally unlocked with its protection password. Changes to protection or key wrapping SHALL preserve coherent ciphertext/domain variants and invalidate affected unlocked sessions before applying replacements. Concurrent protection changes SHALL preserve recoverable variants rather than combining incompatible key materials and ciphertext.

#### Scenario: Exchange a protected tree while it is locked
- **WHEN** a locked protected tree is selected by full-workspace exchange
- **THEN** its ciphertext and wrapped key metadata transfer without requiring unlock and the destination requires the original protection password to read it

#### Scenario: Source is unlocked during exchange
- **WHEN** an unlocked laptop synchronizes protected pages to a phone
- **THEN** synchronization uses persistent ciphertext and the phone receives no decrypted content or session keys

#### Scenario: Concurrent protection changes
- **WHEN** both replicas change protection metadata or one rekeys while the other edits ciphertext under an older domain
- **THEN** coherent variants are retained as a protected conflict, stale unlock sessions are cleared, and no variant is silently rendered unrecoverable

#### Scenario: Protection races with an ordinary edit
- **WHEN** one replica protects a tree while another edits its previously ordinary content and no local protection key is available
- **THEN** that dependency group remains pending with the original revisions preserved on their sources until local unlock permits encrypted conflict storage, and the receiver creates no new plaintext conflict body for the protected tree

### Requirement: Encrypted protected titles and unlock-time migration

The system SHALL encrypt protected page titles under their domain DSK and record an explicit ciphertext flag. It SHALL migrate legacy protected titles transactionally following successful local password unlock. It SHALL withhold unmigrated protected trees from exchange and use neutral locked navigation labels. Removing protection SHALL decrypt titles locally; moving a page SHALL transform its title with its content into the target domain.

#### Scenario: Unlock an existing protected tree
- **WHEN** a password successfully unlocks a protected tree containing legacy plaintext titles
- **THEN** its titles are encrypted transactionally before replication eligibility and only decrypted locally for unlocked views

#### Scenario: Exchange before legacy title migration
- **WHEN** a protected tree still contains a legacy plaintext title
- **THEN** that tree remains pending without transferring the title, while independent data may exchange
