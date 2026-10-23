//! Merchant registry.
//!
//! The registry is the source of truth the payment processor consults before
//! it moves any funds: a merchant must be registered, approved by the registry
//! admin, not suspended, and must list the asset being paid in.
//!
//! Lifecycle: `register` (Pending) -> `approve` (Active) <-> `suspend`
//! (Suspended) -> `reinstate` (Active). Merchants manage their own settlement
//! account and accepted-asset list; only the admin changes status.

#![no_std]

use shared::{extend_instance, extend_persistent};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, String, Vec,
};

/// Maximum number of assets a merchant can accept. Keeps `accepts` cheap.
pub const MAX_ACCEPTED_ASSETS: u32 = 10;
pub const MAX_NAME_LEN: u32 = 64;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RegistryError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Paused = 3,
    AlreadyRegistered = 4,
    MerchantNotFound = 5,
    InvalidName = 6,
    TooManyAssets = 7,
    DuplicateAsset = 8,
    InvalidStatusTransition = 9,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MerchantStatus {
    Pending,
    Active,
    Suspended,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Merchant {
    /// Account that controls the merchant profile.
    pub owner: Address,
    pub name: String,
    /// Account that receives net sale proceeds. May differ from `owner`
    /// (e.g. a cold wallet or a business bank anchor account).
    pub settlement: Address,
    pub accepted_assets: Vec<Address>,
    pub status: MerchantStatus,
    pub registered_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Paused,
    MerchantCount,
    Merchant(Address),
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantRegistered {
    #[topic]
    pub owner: Address,
    pub name: String,
    pub settlement: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantStatusChanged {
    #[topic]
    pub owner: Address,
    pub status: MerchantStatus,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantUpdated {
    #[topic]
    pub owner: Address,
    pub settlement: Address,
    pub accepted_assets: Vec<Address>,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseToggled {
    pub paused: bool,
}

#[contract]
pub struct MerchantRegistry;

#[contractimpl]
impl MerchantRegistry {
    pub fn initialize(env: Env, admin: Address) -> Result<(), RegistryError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(RegistryError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::MerchantCount, &0u32);
        extend_instance(&env);
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Merchant self-service
    // ---------------------------------------------------------------------

    /// Registers the caller as a merchant in `Pending` status.
    pub fn register(
        env: Env,
        owner: Address,
        name: String,
        settlement: Address,
        accepted_assets: Vec<Address>,
    ) -> Result<(), RegistryError> {
        Self::require_not_paused(&env)?;
        owner.require_auth();

        let key = DataKey::Merchant(owner.clone());
        if env.storage().persistent().has(&key) {
            return Err(RegistryError::AlreadyRegistered);
        }
        Self::validate_name(&name)?;
        Self::validate_assets(&accepted_assets)?;

        let now = env.ledger().timestamp();
        let merchant = Merchant {
            owner: owner.clone(),
            name: name.clone(),
            settlement: settlement.clone(),
            accepted_assets,
            status: MerchantStatus::Pending,
            registered_at: now,
            updated_at: now,
        };
        env.storage().persistent().set(&key, &merchant);
        extend_persistent(&env, &key);

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MerchantCount)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::MerchantCount, &(count + 1));
        extend_instance(&env);

        MerchantRegistered {
            owner,
            name,
            settlement,
        }
        .publish(&env);
        Ok(())
    }

    /// Updates the settlement account and accepted assets in one call.
    pub fn update_profile(
        env: Env,
        owner: Address,
        settlement: Address,
        accepted_assets: Vec<Address>,
    ) -> Result<(), RegistryError> {
        Self::require_not_paused(&env)?;
        owner.require_auth();
        Self::validate_assets(&accepted_assets)?;

        let mut merchant = Self::load(&env, &owner)?;
        merchant.settlement = settlement.clone();
        merchant.accepted_assets = accepted_assets.clone();
        merchant.updated_at = env.ledger().timestamp();
        Self::save(&env, &merchant);

        MerchantUpdated {
            owner,
            settlement,
            accepted_assets,
        }
        .publish(&env);
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Admin
    // ---------------------------------------------------------------------

    pub fn approve(env: Env, owner: Address) -> Result<(), RegistryError> {
        Self::set_status(&env, owner, MerchantStatus::Pending, MerchantStatus::Active)
    }

    pub fn suspend(env: Env, owner: Address) -> Result<(), RegistryError> {
        Self::set_status(
            &env,
            owner,
            MerchantStatus::Active,
            MerchantStatus::Suspended,
        )
    }

    pub fn reinstate(env: Env, owner: Address) -> Result<(), RegistryError> {
        Self::set_status(
            &env,
            owner,
            MerchantStatus::Suspended,
            MerchantStatus::Active,
        )
    }

    pub fn pause(env: Env) -> Result<(), RegistryError> {
        Self::set_paused(&env, true)
    }

    pub fn unpause(env: Env) -> Result<(), RegistryError> {
        Self::set_paused(&env, false)
    }

    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<(), RegistryError> {
        Self::admin(&env)?.require_auth();
        new_admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        extend_instance(&env);
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Views (consumed by the payment processor and the backend)
    // ---------------------------------------------------------------------

    pub fn get_merchant(env: Env, owner: Address) -> Result<Merchant, RegistryError> {
        Self::load(&env, &owner)
    }

    /// True when the merchant exists and is `Active`.
    pub fn is_active(env: Env, owner: Address) -> bool {
        matches!(
            Self::load(&env, &owner).map(|m| m.status),
            Ok(MerchantStatus::Active)
        )
    }

    /// True when the merchant is active and lists `asset`.
    pub fn accepts(env: Env, owner: Address, asset: Address) -> bool {
        match Self::load(&env, &owner) {
            Ok(m) => m.status == MerchantStatus::Active && m.accepted_assets.contains(&asset),
            Err(_) => false,
        }
    }

    /// Settlement account for an active merchant.
    pub fn settlement_of(env: Env, owner: Address) -> Result<Address, RegistryError> {
        let merchant = Self::load(&env, &owner)?;
        if merchant.status != MerchantStatus::Active {
            return Err(RegistryError::InvalidStatusTransition);
        }
        Ok(merchant.settlement)
    }

    pub fn merchant_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::MerchantCount)
            .unwrap_or(0)
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    pub fn get_admin(env: Env) -> Result<Address, RegistryError> {
        Self::admin(&env)
    }

    // ---------------------------------------------------------------------
    // Internals
    // ---------------------------------------------------------------------

    fn admin(env: &Env) -> Result<Address, RegistryError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(RegistryError::NotInitialized)
    }

    fn require_not_paused(env: &Env) -> Result<(), RegistryError> {
        if Self::is_paused(env.clone()) {
            return Err(RegistryError::Paused);
        }
        Ok(())
    }

    fn set_paused(env: &Env, paused: bool) -> Result<(), RegistryError> {
        Self::admin(env)?.require_auth();
        env.storage().instance().set(&DataKey::Paused, &paused);
        extend_instance(env);
        PauseToggled { paused }.publish(env);
        Ok(())
    }

    fn set_status(
        env: &Env,
        owner: Address,
        from: MerchantStatus,
        to: MerchantStatus,
    ) -> Result<(), RegistryError> {
        Self::admin(env)?.require_auth();
        let mut merchant = Self::load(env, &owner)?;
        if merchant.status != from {
            return Err(RegistryError::InvalidStatusTransition);
        }
        merchant.status = to;
        merchant.updated_at = env.ledger().timestamp();
        Self::save(env, &merchant);
        MerchantStatusChanged { owner, status: to }.publish(env);
        Ok(())
    }

    fn load(env: &Env, owner: &Address) -> Result<Merchant, RegistryError> {
        let key = DataKey::Merchant(owner.clone());
        let merchant = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(RegistryError::MerchantNotFound)?;
        extend_persistent(env, &key);
        Ok(merchant)
    }

    fn save(env: &Env, merchant: &Merchant) {
        let key = DataKey::Merchant(merchant.owner.clone());
        env.storage().persistent().set(&key, merchant);
        extend_persistent(env, &key);
    }

    fn validate_name(name: &String) -> Result<(), RegistryError> {
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            return Err(RegistryError::InvalidName);
        }
        Ok(())
    }

    fn validate_assets(assets: &Vec<Address>) -> Result<(), RegistryError> {
        if assets.len() > MAX_ACCEPTED_ASSETS {
            return Err(RegistryError::TooManyAssets);
        }
        for (i, asset) in assets.iter().enumerate() {
            if let Some(first) = assets.first_index_of(&asset) {
                if first as usize != i {
                    return Err(RegistryError::DuplicateAsset);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test;
