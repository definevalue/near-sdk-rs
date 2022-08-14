use borsh::BorshSchema;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Error, Write};
use std::marker::PhantomData;
use std::rc::Rc;

use crate::{AccountId, Balance, Gas, GasWeight, PromiseIndex, PublicKey};

enum PromiseAction {
    CreateAccount,
    DeployContract {
        code: Vec<u8>,
    },
    FunctionCall {
        function_name: String,
        arguments: Vec<u8>,
        amount: Balance,
        gas: Gas,
    },
    FunctionCallWeight {
        function_name: String,
        arguments: Vec<u8>,
        amount: Balance,
        gas: Gas,
        weight: GasWeight,
    },
    Transfer {
        amount: Balance,
    },
    Stake {
        amount: Balance,
        public_key: PublicKey,
    },
    AddFullAccessKey {
        public_key: PublicKey,
        nonce: u64,
    },
    AddAccessKey {
        public_key: PublicKey,
        allowance: Balance,
        receiver_id: AccountId,
        function_names: String,
        nonce: u64,
    },
    DeleteKey {
        public_key: PublicKey,
    },
    DeleteAccount {
        beneficiary_id: AccountId,
    },
}

impl PromiseAction {
    pub fn add(&self, promise_index: PromiseIndex) {
        use PromiseAction::*;
        match self {
            CreateAccount => crate::env::promise_batch_action_create_account(promise_index),
            DeployContract { code } => {
                crate::env::promise_batch_action_deploy_contract(promise_index, code)
            }
            FunctionCall { function_name, arguments, amount, gas } => {
                crate::env::promise_batch_action_function_call(
                    promise_index,
                    function_name,
                    arguments,
                    *amount,
                    *gas,
                )
            }
            FunctionCallWeight { function_name, arguments, amount, gas, weight } => {
                crate::env::promise_batch_action_function_call_weight(
                    promise_index,
                    function_name,
                    arguments,
                    *amount,
                    *gas,
                    GasWeight(weight.0),
                )
            }
            Transfer { amount } => {
                crate::env::promise_batch_action_transfer(promise_index, *amount)
            }
            Stake { amount, public_key } => {
                crate::env::promise_batch_action_stake(promise_index, *amount, public_key)
            }
            AddFullAccessKey { public_key, nonce } => {
                crate::env::promise_batch_action_add_key_with_full_access(
                    promise_index,
                    public_key,
                    *nonce,
                )
            }
            AddAccessKey { public_key, allowance, receiver_id, function_names, nonce } => {
                crate::env::promise_batch_action_add_key_with_function_call(
                    promise_index,
                    public_key,
                    *nonce,
                    *allowance,
                    receiver_id,
                    function_names,
                )
            }
            DeleteKey { public_key } => {
                crate::env::promise_batch_action_delete_key(promise_index, public_key)
            }
            DeleteAccount { beneficiary_id } => {
                crate::env::promise_batch_action_delete_account(promise_index, beneficiary_id)
            }
        }
    }
}

struct PromiseSingle {
    pub account_id: AccountId,
    pub actions: RefCell<Vec<PromiseAction>>,
    pub after: RefCell<Option<PromiseIndex>>,
    /// Promise index that is computed only once.
    pub promise_index: RefCell<Option<PromiseIndex>>,
}

impl PromiseSingle {
    pub fn construct_recursively(&self) -> PromiseIndex {
        let mut promise_lock = self.promise_index.borrow_mut();
        if let Some(res) = promise_lock.as_ref() {
            return *res;
        }
        let promise_index = if let Some(after) = self.after.borrow().as_ref() {
            crate::env::promise_batch_then(*after, &self.account_id)
        } else {
            crate::env::promise_batch_create(&self.account_id)
        };
        let actions_lock = self.actions.borrow();
        for action in actions_lock.iter() {
            action.add(promise_index);
        }
        *promise_lock = Some(promise_index);
        promise_index
    }
}

pub struct PromiseJoint {
    pub promise_a: PromiseIndex,
    pub promise_b: PromiseIndex,
    /// Promise index that is computed only once.
    pub promise_index: RefCell<Option<PromiseIndex>>,
}

impl PromiseJoint {
    pub fn construct_recursively(&self) -> PromiseIndex {
        let mut promise_lock = self.promise_index.borrow_mut();
        if let Some(res) = promise_lock.as_ref() {
            return *res;
        }
        let res = crate::env::promise_and(&[self.promise_a, self.promise_b]);
        *promise_lock = Some(res);
        res
    }
}

/// A structure representing a result of the scheduled execution on another contract.
///
/// Smart contract developers will explicitly use `Promise` in two situations:
/// * When they need to return `Promise`.
///
///   In the following code if someone calls method `ContractA::a` they will internally cause an
///   execution of method `ContractB::b` of `bob_near` account, and the return value of `ContractA::a`
///   will be what `ContractB::b` returned.
/// ```no_run
/// # use near_sdk::{ext_contract, near_bindgen, Promise, Gas};
/// # use borsh::{BorshDeserialize, BorshSerialize};
/// #[ext_contract]
/// pub trait ContractB {
///     fn b(&mut self);
/// }
///
/// #[near_bindgen]
/// #[derive(Default, BorshDeserialize, BorshSerialize)]
/// struct ContractA {}
///
/// #[near_bindgen]
/// impl ContractA {
///     pub fn a(&self) -> Promise {
///         contract_b::ext("bob_near".parse().unwrap()).b()
///     }
/// }
/// ```
///
/// * When they need to create a transaction with one or many actions, e.g. the following code
///   schedules a transaction that creates an account, transfers tokens, and assigns a public key:
///
/// ```no_run
/// # use near_sdk::{Promise, env, test_utils::VMContextBuilder, testing_env};
/// # testing_env!(VMContextBuilder::new().signer_account_id("bob_near".parse().unwrap())
/// #               .account_balance(1000).prepaid_gas(1_000_000.into()).build());
/// Promise::new("bob_near".parse().unwrap())
///   .create_account()
///   .transfer(1000)
///   .add_full_access_key(env::signer_account_pk());
/// ```
pub struct Promise<T = ()> {
    subtype: PromiseSubtype,
    should_return: RefCell<bool>,
    _marker: PhantomData<fn() -> T>,
}

/// Until we implement strongly typed promises we serialize them as unit struct.
impl<T> BorshSchema for Promise<T>
where
    T: BorshSchema,
{
    fn add_definitions_recursively(
        definitions: &mut HashMap<borsh::schema::Declaration, borsh::schema::Definition>,
    ) {
        <T>::add_definitions_recursively(definitions);
    }

    fn declaration() -> borsh::schema::Declaration {
        <T>::declaration()
    }
}

#[derive(Clone)]
enum PromiseSubtype {
    Single(Rc<PromiseSingle>),
    Joint(Rc<PromiseJoint>),
}

impl Promise<()> {
    pub fn new(account_id: AccountId) -> Self {
        Self::new_with_return(account_id)
    }
}

impl<T> Promise<T> {
    /// Create a promise that acts on the given account.
    // TODO this is bad because it requires generic on a non-function call
    pub fn new_with_return(account_id: AccountId) -> Self {
        Self {
            subtype: PromiseSubtype::Single(Rc::new(PromiseSingle {
                account_id,
                actions: RefCell::new(vec![]),
                after: RefCell::new(None),
                promise_index: RefCell::new(None),
            })),
            should_return: RefCell::new(false),
            _marker: Default::default(),
        }
    }

    fn add_action(self, action: PromiseAction) -> Self {
        match &self.subtype {
            PromiseSubtype::Single(x) => x.actions.borrow_mut().push(action),
            PromiseSubtype::Joint(_) => {
                crate::env::panic_str("Cannot add action to a joint promise.")
            }
        }
        self
    }

    /// Create account on which this promise acts.
    pub fn create_account(self) -> Self {
        self.add_action(PromiseAction::CreateAccount)
    }

    /// Deploy a smart contract to the account on which this promise acts.
    pub fn deploy_contract(self, code: Vec<u8>) -> Self {
        self.add_action(PromiseAction::DeployContract { code })
    }

    /// A low-level interface for making a function call to the account that this promise acts on.
    // TODO not really a way to specify generic on function call, which is how it should be
    pub fn function_call(
        self,
        function_name: String,
        arguments: Vec<u8>,
        amount: Balance,
        gas: Gas,
    ) -> Self {
        self.add_action(PromiseAction::FunctionCall { function_name, arguments, amount, gas })
    }

    /// A low-level interface for making a function call to the account that this promise acts on.
    /// unlike [`Promise::function_call`], this function accepts a weight to use relative unused gas
    /// on this function call at the end of the scheduling method execution.
    pub fn function_call_weight(
        self,
        function_name: String,
        arguments: Vec<u8>,
        amount: Balance,
        gas: Gas,
        weight: GasWeight,
    ) -> Self {
        self.add_action(PromiseAction::FunctionCallWeight {
            function_name,
            arguments,
            amount,
            gas,
            weight,
        })
    }

    /// Transfer tokens to the account that this promise acts on.
    pub fn transfer(self, amount: Balance) -> Self {
        self.add_action(PromiseAction::Transfer { amount })
    }

    /// Stake the account for the given amount of tokens using the given public key.
    pub fn stake(self, amount: Balance, public_key: PublicKey) -> Self {
        self.add_action(PromiseAction::Stake { amount, public_key })
    }

    /// Add full access key to the given account.
    pub fn add_full_access_key(self, public_key: PublicKey) -> Self {
        self.add_full_access_key_with_nonce(public_key, 0)
    }

    /// Add full access key to the given account with a provided nonce.
    pub fn add_full_access_key_with_nonce(self, public_key: PublicKey, nonce: u64) -> Self {
        self.add_action(PromiseAction::AddFullAccessKey { public_key, nonce })
    }

    /// Add an access key that is restricted to only calling a smart contract on some account using
    /// only a restricted set of methods. Here `function_names` is a comma separated list of methods,
    /// e.g. `"method_a,method_b".to_string()`.
    pub fn add_access_key(
        self,
        public_key: PublicKey,
        allowance: Balance,
        receiver_id: AccountId,
        function_names: String,
    ) -> Self {
        self.add_access_key_with_nonce(public_key, allowance, receiver_id, function_names, 0)
    }

    /// Add an access key with a provided nonce.
    pub fn add_access_key_with_nonce(
        self,
        public_key: PublicKey,
        allowance: Balance,
        receiver_id: AccountId,
        function_names: String,
        nonce: u64,
    ) -> Self {
        self.add_action(PromiseAction::AddAccessKey {
            public_key,
            allowance,
            receiver_id,
            function_names,
            nonce,
        })
    }

    /// Delete access key from the given account.
    pub fn delete_key(self, public_key: PublicKey) -> Self {
        self.add_action(PromiseAction::DeleteKey { public_key })
    }

    /// Delete the given account.
    pub fn delete_account(self, beneficiary_id: AccountId) -> Self {
        self.add_action(PromiseAction::DeleteAccount { beneficiary_id })
    }

    /// Merge this promise with another promise, so that we can schedule execution of another
    /// smart contract right after all merged promises finish.
    ///
    /// Note, once the promises are merged it is not possible to add actions to them, e.g. the
    /// following code will panic during the execution of the smart contract:
    ///
    /// ```no_run
    /// # use near_sdk::{Promise, testing_env};
    /// let p1 = Promise::new("bob_near".parse().unwrap()).create_account();
    /// let p2 = Promise::new("carol_near".parse().unwrap()).create_account();
    /// let p3 = p1.and(p2);
    /// // p3.create_account();
    /// ```
    pub fn and<O>(self, other: Promise<O>) -> Promise<PromiseAnd<T, O>> {
        Promise {
            subtype: PromiseSubtype::Joint(Rc::new(PromiseJoint {
                promise_a: self.construct_recursively(),
                promise_b: other.construct_recursively(),
                promise_index: RefCell::new(None),
            })),
            should_return: RefCell::new(false),
            _marker: PhantomData::default(),
        }
    }

    /// Schedules execution of another promise right after the current promise finish executing.
    ///
    /// In the following code `bob_near` and `dave_near` will be created concurrently. `carol_near`
    /// creation will wait for `bob_near` to be created, and `eva_near` will wait for both `carol_near`
    /// and `dave_near` to be created first.
    /// ```no_run
    /// # use near_sdk::{Promise, VMContext, testing_env};
    /// let p1 = Promise::new("bob_near".parse().unwrap()).create_account();
    /// let p2 = Promise::new("carol_near".parse().unwrap()).create_account();
    /// let p3 = Promise::new("dave_near".parse().unwrap()).create_account();
    /// let p4 = Promise::new("eva_near".parse().unwrap()).create_account();
    /// p1.then(p2).and(p3).then(p4);
    /// ```
    pub fn then<O>(self, mut other: Promise<O>) -> Promise<O> {
        match &mut other.subtype {
            PromiseSubtype::Single(x) => {
                let mut after = x.after.borrow_mut();
                if after.is_some() {
                    crate::env::panic_str(
                        "Cannot callback promise which is already scheduled after another",
                    );
                }
                *after = Some(self.construct_recursively())
            }
            PromiseSubtype::Joint(_) => crate::env::panic_str("Cannot callback joint promise."),
        }
        other
    }

    /// A specialized, relatively low-level API method. Allows to mark the given promise as the one
    /// that should be considered as a return value.
    ///
    /// In the below code `a1` and `a2` functions are equivalent.
    /// ```
    /// # use near_sdk::{ext_contract, Gas, near_bindgen, Promise};
    /// # use borsh::{BorshDeserialize, BorshSerialize};
    /// #[ext_contract]
    /// pub trait ContractB {
    ///     fn b(&mut self);
    /// }
    ///
    /// #[near_bindgen]
    /// #[derive(Default, BorshDeserialize, BorshSerialize)]
    /// struct ContractA {}
    ///
    /// #[near_bindgen]
    /// impl ContractA {
    ///     pub fn a1(&self) {
    ///        contract_b::ext("bob_near".parse().unwrap()).b().as_return();
    ///     }
    ///
    ///     pub fn a2(&self) -> Promise {
    ///        contract_b::ext("bob_near".parse().unwrap()).b()
    ///     }
    /// }
    /// ```
    #[allow(clippy::wrong_self_convention)]
    pub fn as_return(self) -> Self {
        *self.should_return.borrow_mut() = true;
        self
    }

    fn construct_recursively(&self) -> PromiseIndex {
        let res = match &self.subtype {
            PromiseSubtype::Single(x) => x.construct_recursively(),
            PromiseSubtype::Joint(x) => x.construct_recursively(),
        };
        if *self.should_return.borrow() {
            crate::env::promise_return(res);
        }
        res
    }
}

impl<T> Drop for Promise<T> {
    fn drop(&mut self) {
        self.construct_recursively();
    }
}

impl<T> serde::Serialize for Promise<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        *self.should_return.borrow_mut() = true;
        serializer.serialize_unit()
    }
}

impl<T> borsh::BorshSerialize for Promise<T> {
    fn serialize<W: Write>(&self, _writer: &mut W) -> Result<(), Error> {
        *self.should_return.borrow_mut() = true;

        // Intentionally no bytes written for the promise, the return value from the promise
        // will be considered as the return value from the contract call.
        Ok(())
    }
}

#[derive(serde::Serialize)]
#[serde(untagged)]
pub enum PromiseOrValue<T> {
    Promise(Promise<T>),
    Value(T),
}

impl<T> BorshSchema for PromiseOrValue<T>
where
    T: BorshSchema,
{
    fn add_definitions_recursively(
        definitions: &mut HashMap<borsh::schema::Declaration, borsh::schema::Definition>,
    ) {
        T::add_definitions_recursively(definitions);
    }

    fn declaration() -> borsh::schema::Declaration {
        T::declaration()
    }
}

impl<T> From<Promise<T>> for PromiseOrValue<T> {
    fn from(promise: Promise<T>) -> Self {
        PromiseOrValue::Promise(promise)
    }
}

impl<T: borsh::BorshSerialize> borsh::BorshSerialize for PromiseOrValue<T> {
    fn serialize<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        match self {
            // Only actual value is serialized.
            PromiseOrValue::Value(x) => x.serialize(writer),
            // The promise is dropped to cause env::promise calls.
            PromiseOrValue::Promise(p) => p.serialize(writer),
        }
    }
}

/// Generic type to indicate merged promises that execute in parallel.
///
/// The ordering of return values is in the order of L (left) -> R (right)
/// where the generics act as a binary tree.
pub struct PromiseAnd<L, R> {
    _marker: PhantomData<fn() -> (L, R)>,
}

impl<L, R> BorshSchema for PromiseAnd<L, R>
where
    L: BorshSchema,
    R: BorshSchema,
{
    fn add_definitions_recursively(
        definitions: &mut HashMap<borsh::schema::Declaration, borsh::schema::Definition>,
    ) {
        // TODO this might be able to recursively check the sub declarations, and if they are
        // `PromiseAnd`, then the tuple elements are pulled and flattened into one definition.
        // Currently, with nested `PromiseAnd`, the definition will look like (A, (B, (C, D)))
        // which is usable, but not as clear as it could be.
        Self::add_definition(
            Self::declaration(),
            borsh::schema::Definition::Tuple { elements: vec![L::declaration(), R::declaration()] },
            definitions,
        );
        <L>::add_definitions_recursively(definitions);
        <R>::add_definitions_recursively(definitions);
    }

    fn declaration() -> borsh::schema::Declaration {
        format!("PromiseAnd<{}, {}>", L::declaration(), R::declaration())
    }
}
