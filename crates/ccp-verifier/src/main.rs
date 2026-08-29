use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "ccp-verifier",
    version,
    about = "Verify Commit CI Preflight receipts"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Verify(VerifyArgs),
    Schema(SchemaArgs),
}

#[derive(Debug, Args)]
struct VerifyArgs {
    #[arg(
        long,
        default_value = ".ccp/receipt.json",
        help = "receipt path (default: .ccp/receipt.json)"
    )]
    receipt: PathBuf,
    #[arg(
        long,
        default_value = ".commit-ci-policy.toml",
        help = "policy path (default: .commit-ci-policy.toml)"
    )]
    policy: PathBuf,
    #[arg(long)]
    expected_commit: String,
    #[arg(long)]
    evaluated_at_utc: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SchemaArgs {
    #[arg(long, value_enum)]
    kind: SchemaKind,
}

#[derive(Clone, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum SchemaKind {
    ReceiptV1,
    ReceiptV2,
    PolicyV1,
    PolicyV1_1,
    PolicyV2,
    VerificationReportV1,
}

fn main() {
    let result = match Cli::parse().command {
        Command::Verify(args) => verify(args),
        Command::Schema(args) => schema(args),
    };
    if let Err((code, message)) = result {
        eprintln!("error: {message}");
        std::process::exit(code);
    }
}

fn verify(args: VerifyArgs) -> Result<(), (i32, String)> {
    ccp_core::verify::validate_verification_policy_path(&args.policy)
        .map_err(|error| (2, error.to_string()))?;
    let evaluated = match args.evaluated_at_utc {
        Some(value) => value,
        None => {
            ccp_core::verify::system_evaluated_at_utc().map_err(|error| (2, error.to_string()))?
        }
    };
    let bytes = std::fs::read(&args.receipt);
    let report = match bytes {
        Ok(bytes) => ccp_core::verify::verify_receipt_document_for_policy_path(
            &bytes,
            &args.policy,
            &args.expected_commit,
            &evaluated,
        )
        .map_err(|error| (2, error.to_string()))?,
        Err(_) => ccp_core::verify::receipt_input_failure_report(&args.expected_commit, &evaluated)
            .map_err(|error| (70, error.to_string()))?,
    };
    let output = if args.json {
        let mut bytes = report
            .canonical_bytes()
            .map_err(|error| (70, error.to_string()))?;
        bytes.push(b'\n');
        bytes
    } else {
        let mut text = format!(
            "Integrity: {:?}\nPolicy: {:?}\n",
            report.integrity_status, report.policy_status
        );
        for finding in &report.findings {
            text.push_str(&format!(
                "  - {} [{}]: {}\n",
                finding.code, finding.field, finding.message
            ));
        }
        text.push_str(&format!("Decision: {:?}\n", report.decision));
        text.into_bytes()
    };
    print_bytes(&output).map_err(|error| (70, error.to_string()))?;
    if report.decision == ccp_core::verify::VerificationDecision::Pass {
        Ok(())
    } else {
        Err((3, "verification completed with Fail".to_owned()))
    }
}

fn schema(args: SchemaArgs) -> Result<(), (i32, String)> {
    let result = match args.kind {
        SchemaKind::ReceiptV1 => {
            ccp_core::receipt::receipt_schema_json().map_err(|e| e.to_string())
        }
        SchemaKind::ReceiptV2 => {
            ccp_core::schema::combined_receipt_v2_schema_json().map_err(|e| e.to_string())
        }
        SchemaKind::PolicyV1 => {
            ccp_core::verify::verification_policy_schema_json().map_err(|e| e.to_string())
        }
        SchemaKind::PolicyV1_1 => {
            ccp_core::verify::trusted_plan_policy_schema_json().map_err(|e| e.to_string())
        }
        SchemaKind::PolicyV2 => {
            ccp_core::schema::matrix_policy_schema_json().map_err(|e| e.to_string())
        }
        SchemaKind::VerificationReportV1 => {
            ccp_core::verify::verification_report_schema_json().map_err(|e| e.to_string())
        }
    }
    .map_err(|error| (70, error))?;
    print_bytes(result.as_bytes()).map_err(|error| (70, error.to_string()))
}

fn print_bytes(bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    std::io::stdout().write_all(bytes)
}
