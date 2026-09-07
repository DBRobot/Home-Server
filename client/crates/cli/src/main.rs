use anyhow::Result;
use ente::{AuthFlow, AuthFlowUi, LoginParams, OtpPurpose, SecondFactorMethod, TotpPurpose};
use zeroize::Zeroizing;

struct Cli;

fn prompt(label: &str) -> ente::Result<String> {
    use std::io::Write;
    print!("{label}: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    Ok(line.trim().to_string())
}

impl AuthFlowUi for Cli {
    fn read_email_otp(
        &mut self,
        email: &str,
        _purpose: OtpPurpose,
        _resent: bool,
    ) -> ente::Result<String> {
        prompt(&format!("code emailed to {email}"))
    }

    fn read_totp_code(&mut self, _purpose: TotpPurpose) -> ente::Result<String> {
        prompt("authenticator code")
    }

    fn report_retryable_error(&mut self, message: &str) -> ente::Result<()> {
        eprintln!("retrying: {message}");
        Ok(())
    }

    fn choose_second_factor(
        &mut self,
        methods: &[SecondFactorMethod],
    ) -> ente::Result<SecondFactorMethod> {
        Ok(methods[0])
    }

    fn present_passkey_verification(&mut self, url: &str) -> ente::Result<()> {
        println!("open to verify: {url}");
        Ok(())
    }

    fn wait_for_passkey_verification(&mut self) -> ente::Result<()> {
        prompt("press enter once verified").map(|_| ())
    }

    fn present_totp_secret(&mut self, secret_code: &str, _qr: &str) -> ente::Result<()> {
        println!("totp secret: {secret_code}");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let origin = args
        .next()
        .unwrap_or_else(|| "https://api.distributed-datacenter.duckdns.org".to_string());
    let email = args.next().unwrap_or_default();
    let password = Zeroizing::new(rpassword::prompt_password("ente password: ")?);

    let client = ente::client(&origin)?;
    let mut ui = Cli;
    let mut flow = AuthFlow::new(&client, &mut ui);

    let account = flow.login(LoginParams { email, password }).await?;

    println!("logged in, user_id {}", account.user_id);
    Ok(())
}
