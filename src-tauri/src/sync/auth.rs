//! A provisional TLS connection is authorized only after channel-bound PAKE
//! and role-separated mutual key confirmation. No workspace messages precede it.
use crate::error::{VaultError, VaultResult};
use hmac::{Hmac, Mac};
use opaque_ke::{
    CipherSuite, ClientLogin, ClientLoginFinishParameters, ClientRegistration,
    ClientRegistrationFinishParameters, CredentialFinalization, CredentialRequest,
    CredentialResponse, Identifiers, ServerLogin, ServerLoginParameters, ServerRegistration,
    ServerSetup,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, ServerConfig, ServerConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

pub const PROTOCOL: &str = "tenjee-lan-v1-opaque-ristretto255-sha512-tls13-entities2";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Protocol { #[default] Vault, Files }
impl Protocol {
    pub fn id(self) -> &'static str { match self { Self::Vault => PROTOCOL, Self::Files => "tenjee-file-v1-opaque-ristretto255-sha512-tls13" } }
    fn exporter(self) -> &'static [u8] { match self { Self::Vault => b"EXPORTER-tenjee-pairing-v1", Self::Files => b"EXPORTER-tenjee-file-pairing-v1" } }
}
const MAX_AUTH_FRAME: usize = 2048;
pub const MAX_FRAME: usize = 16 * 1024 * 1024;
type Confirmation = Hmac<Sha256>;

fn invalid(message: &str) -> VaultError {
    VaultError::Validation(message.into())
}

pub fn send<T: Serialize>(stream: &mut impl Write, message: &T) -> VaultResult<()> {
    let bytes =
        serde_json::to_vec(message).map_err(|_| invalid("Cannot encode exchange message"))?;
    if bytes.len() > MAX_FRAME {
        return Err(invalid("Exchange message exceeds size limit"));
    }
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

pub fn receive<T: serde::de::DeserializeOwned>(
    stream: &mut impl Read,
    maximum: usize,
) -> VaultResult<T> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > maximum {
        return Err(invalid("Invalid exchange message size"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|_| invalid("Invalid exchange message"))
}

#[derive(Debug)]
struct ProvisionalVerifier;
impl ServerCertVerifier for ProvisionalVerifier {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if cert.len() > 8192 || !intermediates.is_empty() {
            return Err(rustls::Error::General(
                "Invalid temporary certificate".into(),
            ));
        }
        // Explicitly provisional, NOT peer trust. The PAKE authenticates both
        // this certificate's fingerprint and the TLS exporter below.
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not supported".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[derive(Clone)]
pub struct ServerIdentity {
    pub config: Arc<ServerConfig>,
    fingerprint: String,
}
impl ServerIdentity {
    pub fn new() -> VaultResult<Self> {
        let identity = rcgen::generate_simple_self_signed(vec!["tenjee.local".into()])
            .map_err(|_| invalid("Cannot create temporary exchange identity"))?;
        let cert = identity.cert.der().clone();
        let fingerprint = crate::blob_store::hash_hex(cert.as_ref());
        let key = PrivatePkcs8KeyDer::from(identity.signing_key.serialize_der());
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .map_err(|_| invalid("TLS configuration failed"))?
                .with_no_client_auth()
                .with_single_cert(vec![cert], key.into())
                .map_err(|_| invalid("TLS identity failed"))?;
        Ok(Self {
            config: Arc::new(config),
            fingerprint,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct Greeting {
    protocol: String,
    session: String,
}
#[derive(Serialize, Deserialize)]
struct PublicMessage {
    message: Vec<u8>,
}
#[derive(Serialize, Deserialize)]
struct Proof {
    proof: Vec<u8>,
}

struct Suite;
impl CipherSuite for Suite {
    type OprfCs = opaque_ke::Ristretto255;
    type KeyExchange = opaque_ke::TripleDh<opaque_ke::Ristretto255, sha2::Sha512>;
    type Ksf = argon2::Argon2<'static>;
}
fn identities() -> Identifiers<'static> {
    Identifiers {
        client: Some(b"tenjee-connector-v1"),
        server: Some(b"tenjee-receiver-v1"),
    }
}

fn proof(protocol: Protocol, key: &[u8], role: &[u8], transcript: &[u8]) -> Vec<u8> {
    let mut mac = Confirmation::new_from_slice(key).expect("HMAC accepts arbitrary key sizes");
    mac.update(protocol.id().as_bytes());
    mac.update(role);
    mac.update(transcript);
    mac.finalize().into_bytes().to_vec()
}

fn verify(protocol: Protocol, key: &[u8], role: &[u8], transcript: &[u8], received: &[u8]) -> VaultResult<()> {
    let mut mac = Confirmation::new_from_slice(key).map_err(|_| invalid("Pairing failed"))?;
    mac.update(protocol.id().as_bytes());
    mac.update(role);
    mac.update(transcript);
    mac.verify_slice(received)
        .map_err(|_| invalid("Pairing failed: check the temporary code"))
}

fn transcript(protocol: Protocol, session: &str, fingerprint: &str, exporter: &[u8], a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut transcript = Vec::new();
    for value in [
        protocol.id().as_bytes(),
        session.as_bytes(),
        fingerprint.as_bytes(),
        exporter,
        a,
        b,
    ] {
        transcript.extend_from_slice(&(value.len() as u32).to_be_bytes());
        transcript.extend_from_slice(value);
    }
    transcript
}

pub fn server_for(
    protocol: Protocol,
    socket: TcpStream,
    identity: &ServerIdentity,
    session: &str,
    code: &str,
) -> VaultResult<StreamOwned<ServerConnection, TcpStream>> {
    if code.len() != 8
        || !code.bytes().all(|byte| byte.is_ascii_digit())
        || uuid::Uuid::parse_str(session).is_err()
    {
        return Err(invalid("Invalid temporary pairing credentials"));
    }
    socket.set_read_timeout(Some(Duration::from_secs(10)))?;
    socket.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut conn = ServerConnection::new(identity.config.clone())
        .map_err(|_| invalid("TLS connection failed"))?;
    let mut socket = socket;
    while conn.is_handshaking() {
        conn.complete_io(&mut socket)?;
    }
    let exporter = conn
        .export_keying_material(
            [0; 32],
            protocol.exporter(),
            Some(session.as_bytes()),
        )
        .map_err(|_| invalid("TLS channel binding failed"))?;
    let mut stream = StreamOwned::new(conn, socket);
    send(
        &mut stream,
        &Greeting {
            protocol: protocol.id().into(),
            session: session.into(),
        },
    )?;
    let binding = transcript(protocol, session, &identity.fingerprint, &exporter, &[], &[]);
    let mut rng = rand::rngs::OsRng;
    // The temporary verifier is created locally. Registration is never exposed
    // on the network and neither the password file nor setup is persisted.
    let setup = ServerSetup::<Suite>::new(&mut rng);
    let registration = ClientRegistration::<Suite>::start(&mut rng, code.as_bytes())
        .map_err(|_| invalid("Pairing failed"))?;
    let response =
        ServerRegistration::<Suite>::start(&setup, registration.message, session.as_bytes())
            .map_err(|_| invalid("Pairing failed"))?;
    let registered = registration
        .state
        .finish(
            &mut rng,
            code.as_bytes(),
            response.message,
            ClientRegistrationFinishParameters::new(identities(), None),
        )
        .map_err(|_| invalid("Pairing failed"))?;
    let _export_key = Zeroizing::new(registered.export_key);
    let password_file = ServerRegistration::<Suite>::finish(registered.message);
    let incoming: PublicMessage = receive(&mut stream, MAX_AUTH_FRAME)?;
    let request = CredentialRequest::<Suite>::deserialize(&incoming.message)
        .map_err(|_| invalid("Invalid pairing message"))?;
    let login = ServerLogin::<Suite>::start(
        &mut rng,
        &setup,
        Some(password_file),
        request,
        session.as_bytes(),
        ServerLoginParameters {
            context: Some(&binding),
            identifiers: identities(),
        },
    )
    .map_err(|_| invalid("Pairing failed"))?;
    let message = login.message.serialize().to_vec();
    send(
        &mut stream,
        &PublicMessage {
            message: message.clone(),
        },
    )?;
    let final_message: PublicMessage = receive(&mut stream, MAX_AUTH_FRAME)?;
    let result = login
        .state
        .finish(
            CredentialFinalization::<Suite>::deserialize(&final_message.message)
                .map_err(|_| invalid("Invalid pairing message"))?,
            ServerLoginParameters {
                context: Some(&binding),
                identifiers: identities(),
            },
        )
        .map_err(|_| invalid("Pairing failed"))?;
    let key = Zeroizing::new(result.session_key);
    let transcript = transcript(
        protocol,
        session,
        &identity.fingerprint,
        &exporter,
        &incoming.message,
        &message,
    );
    let incoming: Proof = receive(&mut stream, MAX_AUTH_FRAME)?;
    verify(protocol, &key, b"connector", &transcript, &incoming.proof)?;
    send(
        &mut stream,
        &Proof {
            proof: proof(protocol, &key, b"receiver", &transcript),
        },
    )?;
    Ok(stream)
}

pub fn client_for(
    protocol: Protocol,
    socket: TcpStream,
    code: &str,
) -> VaultResult<StreamOwned<ClientConnection, TcpStream>> {
    if code.len() != 8 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid("Enter the eight-digit authentication code"));
    }
    socket.set_read_timeout(Some(Duration::from_secs(10)))?;
    socket.set_write_timeout(Some(Duration::from_secs(10)))?;
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| invalid("TLS configuration failed"))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(ProvisionalVerifier))
            .with_no_client_auth();
    let mut conn = ClientConnection::new(
        Arc::new(config),
        ServerName::try_from("tenjee.local").unwrap(),
    )
    .map_err(|_| invalid("TLS connection failed"))?;
    let mut socket = socket;
    while conn.is_handshaking() {
        conn.complete_io(&mut socket)?;
    }
    let fingerprint = crate::blob_store::hash_hex(
        conn.peer_certificates()
            .and_then(|certs| certs.first())
            .ok_or_else(|| invalid("Missing temporary certificate"))?
            .as_ref(),
    );
    let mut stream = StreamOwned::new(conn, socket);
    let greeting: Greeting = receive(&mut stream, MAX_AUTH_FRAME)?;
    if greeting.protocol != protocol.id() || uuid::Uuid::parse_str(&greeting.session).is_err() {
        return Err(invalid("Incompatible exchange mode or protocol. Select the same mode on both devices and use compatible builds"));
    }
    let exporter = stream
        .conn
        .export_keying_material(
            [0; 32],
            protocol.exporter(),
            Some(greeting.session.as_bytes()),
        )
        .map_err(|_| invalid("TLS channel binding failed"))?;
    let binding = transcript(protocol, &greeting.session, &fingerprint, &exporter, &[], &[]);
    let mut rng = rand::rngs::OsRng;
    let login = ClientLogin::<Suite>::start(&mut rng, code.as_bytes())
        .map_err(|_| invalid("Pairing failed"))?;
    let message = login.message.serialize().to_vec();
    send(
        &mut stream,
        &PublicMessage {
            message: message.clone(),
        },
    )?;
    let incoming: PublicMessage = receive(&mut stream, MAX_AUTH_FRAME)?;
    let result = login
        .state
        .finish(
            &mut rng,
            code.as_bytes(),
            CredentialResponse::<Suite>::deserialize(&incoming.message)
                .map_err(|_| invalid("Invalid pairing message"))?,
            ClientLoginFinishParameters::new(Some(&binding), identities(), None),
        )
        .map_err(|_| invalid("Pairing failed: check the temporary code"))?;
    let _export_key = Zeroizing::new(result.export_key);
    send(
        &mut stream,
        &PublicMessage {
            message: result.message.serialize().to_vec(),
        },
    )?;
    let key = Zeroizing::new(result.session_key);
    let transcript = transcript(
        protocol,
        &greeting.session,
        &fingerprint,
        &exporter,
        &message,
        &incoming.message,
    );
    send(
        &mut stream,
        &Proof {
            proof: proof(protocol, &key, b"connector", &transcript),
        },
    )?;
    let incoming: Proof = receive(&mut stream, MAX_AUTH_FRAME)?;
    verify(protocol, &key, b"receiver", &transcript, &incoming.proof)?;
    Ok(stream)
}

pub fn server(socket: TcpStream, identity: &ServerIdentity, session: &str, code: &str) -> VaultResult<StreamOwned<ServerConnection, TcpStream>> {
    server_for(Protocol::Vault, socket, identity, session, code)
}
pub fn client(socket: TcpStream, code: &str) -> VaultResult<StreamOwned<ClientConnection, TcpStream>> {
    client_for(Protocol::Vault, socket, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching_code_authenticates_tls_and_wrong_code_never_returns_a_stream() {
        for matches in [true, false] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let thread = std::thread::spawn(move || {
                let (socket, _) = listener.accept().unwrap();
                server(
                    socket,
                    &ServerIdentity::new().unwrap(),
                    &uuid::Uuid::new_v4().to_string(),
                    "01234567",
                )
                .is_ok()
            });
            let result = client(
                TcpStream::connect(address).unwrap(),
                if matches { "01234567" } else { "76543210" },
            );
            assert_eq!(result.is_ok(), matches);
            assert_eq!(thread.join().unwrap(), matches);
        }
    }
    #[test]
    fn confirmation_is_bound_to_roles_and_transcript() {
        let key = [7; 32];
        let valid = proof(Protocol::Vault, &key, b"connector", b"session-one");
        assert!(verify(Protocol::Vault, &key, b"connector", b"session-one", &valid).is_ok());
        assert!(verify(Protocol::Vault, &key, b"receiver", b"session-one", &valid).is_err());
        assert!(verify(Protocol::Vault, &key, b"connector", b"session-two", &valid).is_err());
    }
    #[test]
    fn oversized_frames_are_rejected_before_allocating_payload() {
        let bytes = ((MAX_AUTH_FRAME + 1) as u32).to_be_bytes();
        assert!(receive::<Greeting>(&mut &bytes[..], MAX_AUTH_FRAME).is_err());
    }
    #[test]
    fn file_protocol_isolated_from_legacy_vault_and_channel_confirmations() {
        assert_eq!(Protocol::Vault.id(), "tenjee-lan-v1-opaque-ristretto255-sha512-tls13-entities2");
        for (listener_mode,connector_mode,accept) in [(Protocol::Files,Protocol::Files,true),(Protocol::Files,Protocol::Vault,false),(Protocol::Vault,Protocol::Files,false)] {
            let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let address=listener.local_addr().unwrap();
            let worker=std::thread::spawn(move||{let (socket,_)=listener.accept().unwrap();server_for(listener_mode,socket,&ServerIdentity::new().unwrap(),&uuid::Uuid::new_v4().to_string(),"01234567").is_ok()});
            assert_eq!(client_for(connector_mode,TcpStream::connect(address).unwrap(),"01234567").is_ok(),accept);
            assert_eq!(worker.join().unwrap(),accept);
        }
        let key=[7;32];let confirmation=proof(Protocol::Files,&key,b"connector",b"channel-one-session-one");
        assert!(verify(Protocol::Vault,&key,b"connector",b"channel-one-session-one",&confirmation).is_err());
        assert!(verify(Protocol::Files,&key,b"connector",b"channel-two-session-one",&confirmation).is_err());
        assert!(verify(Protocol::Files,&key,b"connector",b"channel-one-session-two",&confirmation).is_err());
        assert!(verify(Protocol::Files,&[8;32],b"connector",b"channel-one-session-one",&confirmation).is_err());
    }

    #[test]
    fn pairing_relay_over_substituted_tls_channels_never_authorizes_content() {
        for reuse_certificate in [false, true] {
            let identity = ServerIdentity::new().unwrap();
            // Reusing the certificate deliberately isolates exporter binding:
            // even possession of this test certificate's key cannot authorize
            // a relayed PAKE on a different TLS connection.
            let relay_identity = if reuse_certificate { identity.clone() } else { ServerIdentity::new().unwrap() };
            let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let upstream_address = upstream.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (socket, _) = upstream.accept().unwrap();
                server_for(Protocol::Files, socket, &identity, &uuid::Uuid::new_v4().to_string(), "01234567").is_ok()
            });
            let downstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let downstream_address = downstream.local_addr().unwrap();
            let relay = std::thread::spawn(move || {
                let (mut socket, _) = downstream.accept().unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut connection = ServerConnection::new(relay_identity.config).unwrap();
                while connection.is_handshaking() { connection.complete_io(&mut socket).unwrap(); }
                let mut client_side = StreamOwned::new(connection, socket);
                let config = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                    .with_protocol_versions(&[&rustls::version::TLS13]).unwrap()
                    .dangerous().with_custom_certificate_verifier(Arc::new(ProvisionalVerifier)).with_no_client_auth();
                let mut socket = TcpStream::connect(upstream_address).unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut connection = ClientConnection::new(Arc::new(config), ServerName::try_from("tenjee.local").unwrap()).unwrap();
                while connection.is_handshaking() { connection.complete_io(&mut socket).unwrap(); }
                let mut server_side = StreamOwned::new(connection, socket);
                let greeting: Greeting = receive(&mut server_side, MAX_AUTH_FRAME).unwrap();
                let upstream_exporter = server_side.conn.export_keying_material([0;32], Protocol::Files.exporter(), Some(greeting.session.as_bytes())).unwrap();
                let downstream_exporter = client_side.conn.export_keying_material([0;32], Protocol::Files.exporter(), Some(greeting.session.as_bytes())).unwrap();
                assert_ne!(upstream_exporter, downstream_exporter);
                send(&mut client_side, &greeting).unwrap();
                let request: PublicMessage = receive(&mut client_side, MAX_AUTH_FRAME).unwrap();
                send(&mut server_side, &request).unwrap();
                let response: PublicMessage = receive(&mut server_side, MAX_AUTH_FRAME).unwrap();
                send(&mut client_side, &response).unwrap();
                // The real connector rejects the relayed OPAQUE response and
                // closes before finalization, confirmation, or any offer/body.
                assert!(receive::<serde_json::Value>(&mut client_side, MAX_AUTH_FRAME).is_err());
                drop(server_side);
            });
            assert!(client_for(Protocol::Files, TcpStream::connect(downstream_address).unwrap(), "01234567").is_err());
            relay.join().unwrap();
            assert!(!server.join().unwrap());
        }
    }

}
