use std::{env, path::Path, process::Command};

fn git_output(manifest_dir: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=HARBINGER_GIT_COMMIT");

    let manifest_dir_path = env::var("CARGO_MANIFEST_DIR").expect("manifest directory");
    let manifest_dir = Path::new(&manifest_dir_path);
    if let Some(head) = git_output(manifest_dir, &["rev-parse", "--git-path", "HEAD"]) {
        let head = Path::new(&head);
        println!(
            "cargo:rerun-if-changed={}",
            if head.is_absolute() {
                head.to_path_buf()
            } else {
                manifest_dir.join(head)
            }
            .display()
        );
    }
    if let Some(reference) = git_output(manifest_dir, &["symbolic-ref", "-q", "HEAD"]) {
        if let Some(reference_path) =
            git_output(manifest_dir, &["rev-parse", "--git-path", &reference])
        {
            let reference_path = Path::new(&reference_path);
            println!(
                "cargo:rerun-if-changed={}",
                if reference_path.is_absolute() {
                    reference_path.to_path_buf()
                } else {
                    manifest_dir.join(reference_path)
                }
                .display()
            );
        }
    }

    let commit = env::var("HARBINGER_GIT_COMMIT")
        .ok()
        .filter(|value| value.chars().all(|character| character.is_ascii_hexdigit()))
        .or_else(|| git_output(manifest_dir, &["rev-parse", "--short=12", "HEAD"]))
        .filter(|value| value.len() >= 7)
        .map(|value| value.chars().take(7).collect::<String>())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=HARBINGER_GIT_COMMIT={commit}");
}
