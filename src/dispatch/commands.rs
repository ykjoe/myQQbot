/// Command dispatcher via external Python process.
///
/// Spawns `scripts/command_handler.py` as a long-lived subprocess and
/// communicates with it over stdin/stdout using JSON lines.
/// This allows modifying commands without recompiling the Rust binary.
///
/// If the Python process crashes or the pipe breaks, it is automatically
/// restarted on the next `match_command` call.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{LazyLock, Mutex};

fn spawn_handler() -> Child {
    Command::new("python3")
        .arg("scripts/command_handler.py")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Failed to spawn scripts/command_handler.py")
}

static HANDLER: LazyLock<Mutex<Child>> = LazyLock::new(|| {
    Mutex::new(spawn_handler())
});

/// Try to restart the Python handler process.
fn restart_handler(child: &mut Child) {
    // Kill the old process if still alive
    let _ = child.kill();
    let _ = child.wait();
    *child = spawn_handler();
    println!("[INFO in commands] Python handler restarted.");
}

/// Send a JSON request to the Python handler and return the reply.
///
/// Returns `None` if no command matched or communication failed.
pub fn match_command(content: &str, node_name: &str) -> Option<String> {
    let mut child = HANDLER.lock().ok()?;

    // Build JSON request: {"content": "...", "node_name": "..."}
    let request = serde_json::json!({
        "content": content,
        "node_name": node_name,
    });

    // Write request line to stdin
    let write_ok = {
        let stdin = child.stdin.as_mut()?;
        writeln!(stdin, "{}", request).is_ok() && stdin.flush().is_ok()
    };

    if !write_ok {
        // Pipe broken — Python process likely crashed, restart it
        println!("[WARN in commands] Python handler pipe broken, restarting...");
        restart_handler(&mut child);
        // Retry once
        let stdin = child.stdin.as_mut()?;
        if writeln!(stdin, "{}", request).is_err() || stdin.flush().is_err() {
            println!("[ERROR in commands] Failed to write to restarted Python handler.");
            return None;
        }
    }

    // Read response line from stdout
    let mut line = String::new();
    let read_ok = {
        let stdout = child.stdout.as_mut()?;
        let mut reader = BufReader::new(stdout);
        reader.read_line(&mut line).is_ok()
    };

    if !read_ok {
        // Read failed — restart and give up this request
        println!("[WARN in commands] Python handler read failed, restarting...");
        restart_handler(&mut child);
        return None;
    }

    // Parse JSON response: {"reply": "..."} or {"reply": null}
    let resp: serde_json::Value = serde_json::from_str(&line).ok()?;
    match resp.get("reply")? {
        serde_json::Value::String(s) => Some(s.clone()),
        _ => None,
    }
}
