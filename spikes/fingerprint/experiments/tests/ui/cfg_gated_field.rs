use fingerprint_derive::ContractFingerprint;

#[derive(ContractFingerprint)]
pub struct WithCfgTrue {
    pub always: u32,
    #[cfg(all())]
    pub gated: u32,
}

fn main() {}
