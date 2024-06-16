use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;

use crate::*;

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
#[serde(untagged)]
enum TokenReceiverMessage {
    RequestReview {
        vapi_id: String,
        version: String,
        reviewer_ids: Vec<AccountId>,
        royalty_amounts: Vec<U128>,
        signature: String,
    },
    Settlement {
        vapi_ids: Vec<String>,
        amounts: Vec<U128>,
    },
    Deposit {
        vapi_id: String,
    },
}

#[near]
impl FungibleTokenReceiver for TicleCore {
    fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
        let token_id: AccountId = env::predecessor_account_id();
        require!(token_id == self.token_id, "Invalid token");

        if msg.is_empty() {
            return PromiseOrValue::Value(U128(0));
        }

        log!("[ft_on_transfer] sender_id: {}", sender_id);
        log!("[ft_on_transfer] msg: {}", msg);
        let message = serde_json::from_str::<TokenReceiverMessage>(&msg).expect("Invalid message format");
        log!("[ft_on_transfer] selected message");

        match message {
            TokenReceiverMessage::Deposit { vapi_id } => {
                self.internal_deposit(&sender_id, vapi_id, amount.into());
            }
            TokenReceiverMessage::Settlement { vapi_ids, amounts } => {
                self.internal_settlement(&sender_id, vapi_ids, amounts);
            }
            TokenReceiverMessage::RequestReview { vapi_id, version, reviewer_ids, royalty_amounts, signature } => {
                self.internal_request_review(vapi_id, version, reviewer_ids, royalty_amounts, sender_id, amount.into(), signature);
            }
        }

        return PromiseOrValue::Value(U128(0));
    }
}