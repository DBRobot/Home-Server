use ente::{AuthFlowUi, OtpPurpose, SecondFactorMethod, TotpPurpose};

pub struct Term;

fn prompt(label: &str) -> ente::Result<String> {
    use std::io::Write;
    print!("{label}: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    Ok(line.trim().to_string())
}

impl AuthFlowUi for Term {
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
