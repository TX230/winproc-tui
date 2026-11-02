use std::process::{Command, Output};

fn run_binary(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_winproc-tui"))
        .args(arguments)
        .output()
        .expect("winproc-tui binary should start")
}

#[test]
fn version_reports_the_cargo_package_version() {
    let output = run_binary(&["--version"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        std::str::from_utf8(&output.stdout).unwrap(),
        format!("winproc-tui {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn help_describes_the_binary_entrypoint() {
    let output = run_binary(&["--help"]);

    assert!(output.status.success(), "{output:?}");
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(
        stdout.contains("Windows process investigation TUI"),
        "{stdout}"
    );
    assert!(stdout.contains("Usage: winproc-tui"), "{stdout}");
    assert!(stdout.contains("--help"), "{stdout}");
    assert!(stdout.contains("--version"), "{stdout}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn unknown_option_is_rejected_before_terminal_startup() {
    let output = run_binary(&["--removed-option"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    assert!(stderr.contains("--removed-option"), "{stderr}");
    assert!(stderr.contains("--help"), "{stderr}");
}

#[test]
fn mcp_stdio_negotiates_lists_tools_validates_arguments_and_exits_on_eof() {
    use serde_json::{Value, json};
    use std::{
        io::{BufRead, BufReader, Write},
        process::Stdio,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_winproc-tui"))
            .arg("mcp-bridge")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut input = child.0.stdin.take().unwrap();
    let output = child.0.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut call = |message: Value| {
        writeln!(input, "{message}").unwrap();
        input.flush().unwrap();
        let line = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("MCP reply deadline")
            .unwrap();
        serde_json::from_str::<Value>(&line).expect("stdout contains only JSON")
    };
    let response = call(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"winproc-conformance","version":"1"}}}),
    );
    assert_eq!(response["result"]["protocolVersion"], "2025-11-25");

    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    let mut call = |message: Value| {
        writeln!(input, "{message}").unwrap();
        input.flush().unwrap();
        let line = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("MCP reply deadline")
            .unwrap();
        serde_json::from_str::<Value>(&line).expect("stdout contains only JSON")
    };
    let response = call(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 26);
    let common_output: Value = serde_json::from_str(include_str!(
        "../docs/schemas/mcp-output-envelope.schema.json"
    ))
    .unwrap();
    for tool in tools {
        assert_eq!(tool["outputSchema"], common_output);
        assert!(tool.get("outputDataSchema").is_none());
    }
    let output_validator = jsonschema::validator_for(&common_output).unwrap();
    assert!(
        tools
            .iter()
            .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
    );
    let response = call(
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"winproc_get_metrics","arguments":{"source":"live","unexpected":true}}}),
    );
    assert_eq!(response["result"]["isError"], true);
    assert!(output_validator.is_valid(&response["result"]["structuredContent"]));
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "INVALID_ARGUMENT"
    );
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap(),
        response["result"]["structuredContent"]
    );
    let response = call(json!({"jsonrpc":"2.0","id":4,"method":"ping"}));
    assert_eq!(response["result"], json!({}));
    drop(input);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "bridge must exit on EOF");
        thread::sleep(Duration::from_millis(10));
    }
    reader.join().unwrap();
}
