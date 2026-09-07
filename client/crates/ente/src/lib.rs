pub use ente_accounts::{
    AccountSecrets, AccountsClient, AccountsClientConfig, AuthFlow, AuthFlowUi,
    AuthenticatedAccount, Error, LoginParams, OtpPurpose, Result, SecondFactorMethod, TotpPurpose,
};

pub fn client(origin: &str) -> Result<AccountsClient> {
    AccountsClient::new(AccountsClientConfig {
        origin: origin.to_string(),
        auth_token: None,
        client_package: "org.distributed-datacenter.client".to_string(),
        client_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        user_agent: None,
    })
}
