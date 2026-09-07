use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args
        .next()
        .unwrap_or_else(|| "https://api.distributed-datacenter.duckdns.org".to_string());
    let email = args.next().unwrap_or_default();

    let attrs = ente::srp::srp_attributes(&base, &email).await?;

    println!("srpUserID  {}", attrs.srp_user_id);
    println!("srpSalt    {}", attrs.srp_salt);
    println!("kekSalt    {}", attrs.kek_salt);
    println!(
        "argon2id   memLimit={} opsLimit={}",
        attrs.mem_limit, attrs.ops_limit
    );
    println!("emailMFA   {}", attrs.is_email_mfa_enabled);
    Ok(())
}
