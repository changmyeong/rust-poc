use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;

use crate::*;

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
#[serde(untagged)]
enum TokenReceiverMessage {
    Deposit {
        vapi_id: String,
    },
    Settlement {
        vapi_ids: Vec<String>,
        amounts: Vec<Balance>,
    },
    RequestReview {
        vapi_id: String,
        reviewer_ids: Vec<AccountId>,
        reviewer_infos: Vec<ReviewerInfo>,
    },
}

#[near]
impl FungibleTokenReceiver for Core {
    fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
        let token_id: AccountId = env::predecessor_account_id();
        require!(token_id == self.token_id, "Invalid token");

        if msg.is_empty() {
            PromiseOrValue::Value(U128(0));
        }

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
            TokenReceiverMessage::RequestReview { vapi_id, reviewer_ids, reviewer_infos } => {
                self.internal_request_review(vapi_id, reviewer_ids, reviewer_infos, sender_id, amount.into());
            }
        }

        return PromiseOrValue::Value(U128(0));
    }
}