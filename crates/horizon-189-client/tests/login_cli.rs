//! Exercise the public executable's offline flags through an actual TCP login.
use horizon_protocol_47::{
    Connection,
    packet::{self, NextState},
    write_string,
};
use std::{net::TcpListener, process::Command, time::Duration};

fn check_login(authentication: &[&str]) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "Client never connected"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("Fixture accept failed: {error}"),
            }
        };
        // BSD sockets inherit O_NONBLOCK from the listener on accept.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut connection = Connection::new(stream);
        assert_eq!(
            connection.receive().unwrap(),
            packet::handshake("127.0.0.1", address.port(), NextState::Login).unwrap()
        );
        assert_eq!(connection.receive().unwrap(), [0, 3, b'B', b'o', b'b']);
        connection.send(&[3, 0]).unwrap();
        connection.set_compression(0);
        let mut success = vec![2];
        write_string(&mut success, "b50ad385-829d-3141-a216-7e7d7539ba7f", 36).unwrap();
        write_string(&mut success, "Bob", 16).unwrap();
        connection.send(&success).unwrap();
    });
    let output = Command::new(env!("CARGO_BIN_EXE_horizon-189-client"))
        .args(authentication)
        .args([
            "--login-check",
            "127.0.0.1",
            "--port",
            &address.port().to_string(),
        ])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("PROTOCOL_47_LOGIN_PASS username=Bob")
    );
}

#[test]
fn offline_login_cli_handles_compression_and_success() {
    check_login(&["--offline", "--username", "Bob"]);
}

#[test]
fn direct_token_flags_support_local_testing_without_profile_http() {
    check_login(&[
        "--username",
        "Bob",
        "--uuid",
        "b50ad385829d3141a2167e7d7539ba7f",
        "--access-token",
        "synthetic-local-test-token",
    ]);
}
