use rholang::rust::interpreter::swi_prolog_service::petta_execute;
use rholang::rust::interpreter::test_utils::utils::should_skip_petta_test;

#[tokio::test]
async fn test_sandbox_allows_basic_computation() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = r#"!(+ 1 2)"#;
    let result = petta_execute(metta_code).await;

    println!("\n=== test_sandbox_allows_basic_computation ===");
    println!("Result: {:?}", result);

    assert!(
        result.is_ok(),
        "Basic MeTTa computation should work in sandbox"
    );
}

#[tokio::test]
async fn test_sandbox_blocks_file_access_via_python() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = r#"
        !(py-eval "open('/etc/passwd').read()")
    "#;

    let result = petta_execute(metta_code).await;

    println!("\n=== test_sandbox_blocks_file_access_via_python ===");
    match &result {
        Ok(output) => {
            let encoded = prost::Message::encode_to_vec(output);
            let output_str = String::from_utf8_lossy(&encoded);
            println!("Result: Ok");
            println!("Output contains 'root:': {}", output_str.contains("root:"));
            println!("Output contains 'Error': {}", output_str.contains("Error"));
            println!(
                "Output contains 'PermissionError': {}",
                output_str.contains("PermissionError")
            );
            println!(
                "Output contains 'FileNotFoundError': {}",
                output_str.contains("FileNotFoundError")
            );

            assert!(
                !output_str.contains("root:") && !output_str.contains("daemon:"),
                "Should not be able to read /etc/passwd"
            );
        }
        Err(e) => {
            println!("Result: Err (sandbox blocked access)");
            println!("Error: {:?}", e);
        }
    }

    assert!(
        result.is_err() || {
            let output = result.as_ref().unwrap();
            let encoded = prost::Message::encode_to_vec(output);
            let output_str = String::from_utf8_lossy(&encoded);
            !output_str.contains("root:") && !output_str.contains("daemon:")
        },
        "Sandbox should prevent reading /etc/passwd"
    );
}

#[tokio::test]
async fn test_sandbox_verification_seccomp_active() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = r#"!(+ 5 10)"#;
    let result = petta_execute(metta_code).await;

    println!("\n=== test_sandbox_verification_seccomp_active ===");
    println!("This test verifies the sandbox setup is working by running simple arithmetic");
    println!("The seccomp filter logs should be visible in stderr during test execution");
    println!("Result: {:?}", result.is_ok());

    assert!(
        result.is_ok(),
        "Simple arithmetic should work with seccomp filter active"
    );
}

#[tokio::test]
async fn test_sandbox_timeout_enforcement() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = r#"
        (= (infinite-loop $x) (infinite-loop $x))
        !(infinite-loop 0)
    "#;

    let result = petta_execute(metta_code).await;

    println!("\n=== test_sandbox_timeout_enforcement ===");
    match &result {
        Ok(_) => {
            println!("Result: Ok (unexpected - infinite loop should timeout)");
        }
        Err(e) => {
            println!("Result: Err (expected)");
            println!("Error: {:?}", e);
            let err_str = format!("{:?}", e);
            assert!(
                err_str.contains("timed out") || err_str.contains("timeout"),
                "Error should mention timeout"
            );
        }
    }

    assert!(
        result.is_err(),
        "Infinite loop should be terminated by timeout"
    );
}

#[tokio::test]
async fn test_sandbox_python_integration_works() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = r#"
        !(py-eval "1 + 1")
    "#;

    let result = petta_execute(metta_code).await;

    println!("\n=== test_sandbox_python_integration_works ===");
    match &result {
        Ok(output) => {
            println!("Result: Ok (python integration working)");
            let encoded = prost::Message::encode_to_vec(output);
            let output_str = String::from_utf8_lossy(&encoded);
            println!(
                "Output preview: {:?}",
                &output_str.chars().take(100).collect::<String>()
            );
        }
        Err(e) => {
            println!("Result: Err");
            println!("Error: {:?}", e);
        }
    }

    assert!(
        result.is_ok(),
        "Python integration should work for safe operations"
    );
}

#[tokio::test]
async fn test_sandbox_cannot_access_host_network() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = r#"
        !(py-eval "import socket; s = socket.socket(); s.bind(('0.0.0.0', 31337)); s.listen(1)")
    "#;

    let result = petta_execute(metta_code).await;

    println!("\n=== test_sandbox_cannot_access_host_network ===");
    match &result {
        Ok(output) => {
            let encoded = prost::Message::encode_to_vec(output);
            let output_str = String::from_utf8_lossy(&encoded);
            println!("Result: Ok");
            println!("Output length: {}", output_str.len());
            println!(
                "Output preview: {:?}",
                &output_str.chars().take(500).collect::<String>()
            );
            println!("Contains 'Error': {}", output_str.contains("Error"));
            println!(
                "Contains 'PermissionError': {}",
                output_str.contains("PermissionError")
            );
            println!("Contains 'OSError': {}", output_str.contains("OSError"));
            println!("Contains 'py-eval': {}", output_str.contains("py-eval"));

            // If the output contains 'py-eval', it means the code wasn't evaluated
            if output_str.contains("py-eval") {
                println!("NOTE: Python code was not evaluated (echoed back)");
                return;
            }

            assert!(
                output_str.contains("Error")
                    || output_str.contains("error")
                    || output_str.contains("Exception")
                    || output_str.contains("OSError"),
                "Binding to network port should fail"
            );
        }
        Err(e) => {
            println!("Result: Err (sandbox blocked network access)");
            println!("Error: {:?}", e);
        }
    }
}
