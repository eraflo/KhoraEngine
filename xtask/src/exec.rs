// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not //! Running a child command and reporting its outcome.

use anyhow::Result;
use std::process::Command;
use std::time::Instant;

use crate::term::*;

pub fn execute_command(cmd: &str, args: &[&str], task_name: &str) -> Result<()> {
    let start_time = Instant::now();
    print_command_info(cmd, args);

    let mut command = Command::new(cmd);
    command.args(args);

    let status = command.status()?;
    let duration = start_time.elapsed();

    if status.success() {
        print_success(&format!(
            "{} completed in {:.2}s",
            task_name,
            duration.as_secs_f64()
        ));
        Ok(())
    } else {
        print_error(&format!(
            "{} failed after {:.2}s",
            task_name,
            duration.as_secs_f64()
        ));
        // Using anyhow::bail! is a good way to return an error from a specific point
        anyhow::bail!("{} failed with status: {}", task_name, status);
    }
}
