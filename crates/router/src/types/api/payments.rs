use common_utils::custom_serde;
use error_stack::{IntoReport, ResultExt};
use masking::{PeekInterface, Secret};
use router_derive::Setter;
use time::PrimitiveDateTime;

use super::{ConnectorCommon, RefundResponse};
use crate::{
    core::errors,
    pii,
    services::api,
    types::{self, api as api_types, enums, storage},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaymentOp {
    Create,
    Update,
    Confirm,
}

#[derive(Default, Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PaymentsRequest {
    #[serde(
        default,
        deserialize_with = "crate::utils::custom_serde::payment_id_type::deserialize_option"
    )]
    pub payment_id: Option<PaymentIdType>,
    pub merchant_id: Option<String>,
    pub amount: Option<i32>,
    pub currency: Option<String>,
    pub capture_method: Option<enums::CaptureMethod>,
    pub amount_to_capture: Option<i32>,
    #[serde(default, with = "custom_serde::iso8601::option")]
    pub capture_on: Option<PrimitiveDateTime>,
    pub confirm: Option<bool>,
    pub customer_id: Option<String>,
    pub email: Option<Secret<String, pii::Email>>,
    pub name: Option<Secret<String>>,
    pub phone: Option<Secret<String>>,
    pub phone_country_code: Option<String>,
    pub off_session: Option<bool>,
    pub description: Option<String>,
    pub return_url: Option<String>,
    pub setup_future_usage: Option<enums::FutureUsage>,
    pub authentication_type: Option<enums::AuthenticationType>,
    pub payment_method_data: Option<PaymentMethod>,
    pub payment_method: Option<enums::PaymentMethodType>,
    pub payment_token: Option<i32>,
    pub shipping: Option<Address>,
    pub billing: Option<Address>,
    pub browser_info: Option<types::BrowserInformation>,
    pub statement_descriptor_name: Option<String>,
    pub statement_descriptor_suffix: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub client_secret: Option<String>,
    pub mandate_data: Option<MandateData>,
    pub mandate_id: Option<String>,
}

impl PaymentsRequest {
    pub fn is_mandate(&self) -> Option<MandateTxnType> {
        match (&self.mandate_data, &self.mandate_id) {
            (None, None) => None,
            (_, Some(_)) => Some(MandateTxnType::RecurringMandateTxn),
            (Some(_), _) => Some(MandateTxnType::NewMandateTxn),
        }
    }
}

#[derive(Default, Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PaymentsRedirectRequest {
    pub payment_id: String,
    pub merchant_id: String,
    pub connector: String,
    pub param: String,
}

pub enum MandateTxnType {
    NewMandateTxn,
    RecurringMandateTxn,
}

#[derive(Default, Eq, PartialEq, Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct MandateData {
    pub customer_acceptance: CustomerAcceptance,
}

#[derive(Default, Eq, PartialEq, Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CustomerAcceptance {
    pub acceptance_type: AcceptanceType,
    #[serde(default, with = "custom_serde::iso8601::option")]
    pub accepted_at: Option<PrimitiveDateTime>,
    pub online: Option<OnlineMandate>,
}

impl CustomerAcceptance {
    pub fn get_ip_address(&self) -> Option<String> {
        self.online
            .as_ref()
            .map(|data| data.ip_address.peek().to_owned())
    }
    pub fn get_user_agent(&self) -> Option<String> {
        self.online.as_ref().map(|data| data.user_agent.clone())
    }
    pub fn get_accepted_at(&self) -> PrimitiveDateTime {
        self.accepted_at
            .unwrap_or_else(common_utils::date_time::now)
    }
}

#[derive(Default, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum AcceptanceType {
    Online,
    #[default]
    Offline,
}

#[derive(Default, Eq, PartialEq, Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct OnlineMandate {
    pub ip_address: Secret<String, pii::IpAddress>,
    pub user_agent: String,
}

impl super::Router for PaymentsRequest {}

#[derive(Default, Eq, PartialEq, Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct CCard {
    pub card_number: Secret<String, pii::CardNumber>,
    pub card_exp_month: Secret<String>,
    pub card_exp_year: Secret<String>,
    pub card_holder_name: Secret<String>,
    pub card_cvc: Secret<String>,
}

#[derive(Default, Eq, PartialEq, Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct PayLaterData {
    pub billing_email: String,
    pub country: String,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PaymentMethod {
    #[serde(rename(deserialize = "card"))]
    Card(CCard),
    #[serde(rename(deserialize = "bank_transfer"))]
    BankTransfer,
    Wallet,
    #[serde(rename(deserialize = "pay_later"))]
    PayLater(PayLaterData),
    #[serde(rename(deserialize = "paypal"))]
    Paypal,
}

#[derive(Eq, PartialEq, Clone, Debug, serde::Serialize)]
pub struct CCardResponse {
    last4: String,
    exp_month: String,
    exp_year: String,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize)]
pub enum PaymentMethodDataResponse {
    #[serde(rename = "card")]
    Card(CCardResponse),
    #[serde(rename(deserialize = "bank_transfer"))]
    BankTransfer,
    Wallet,
    PayLater(PayLaterData),
    Paypal,
}

impl Default for PaymentMethod {
    fn default() -> Self {
        PaymentMethod::BankTransfer
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PaymentIdType {
    PaymentIntentId(String),
    ConnectorTransactionId(String),
    PaymentTxnId(String),
}

impl PaymentIdType {
    pub fn get_payment_intent_id(&self) -> errors::CustomResult<String, errors::ValidationError> {
        match self {
            Self::PaymentIntentId(id) => Ok(id.clone()),
            Self::ConnectorTransactionId(_) | Self::PaymentTxnId(_) => {
                Err(errors::ValidationError::IncorrectValueProvided {
                    field_name: "payment_id",
                })
                .into_report()
                .attach_printable("Expected payment intent ID but got connector transaction ID")
            }
        }
    }
}

impl Default for PaymentIdType {
    fn default() -> Self {
        Self::PaymentIntentId(Default::default())
    }
}

// Core related api layer.
#[derive(Debug, Clone)]
pub struct Authorize;
#[derive(Debug, Clone)]
pub struct PCapture;

#[derive(Debug, Clone)]
pub struct PSync;
#[derive(Debug, Clone)]
pub struct Void;

//#[derive(Debug, serde::Deserialize, serde::Serialize)]
//#[serde(untagged)]
//pub enum enums::CaptureMethod {
//Automatic,
//Manual,
//}

//impl Default for enums::CaptureMethod {
//fn default() -> Self {
//enums::CaptureMethod::Manual
//}
//}

#[derive(Default, Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Address {
    pub address: Option<AddressDetails>,
    pub phone: Option<PhoneDetails>,
}

// used by customers also, could be moved outside
#[derive(Clone, Default, Debug, Eq, serde::Deserialize, serde::Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AddressDetails {
    pub city: Option<String>,
    pub country: Option<String>,
    pub line1: Option<Secret<String>>,
    pub line2: Option<Secret<String>>,
    pub line3: Option<Secret<String>>,
    pub zip: Option<Secret<String>>,
    pub state: Option<Secret<String>>,
    pub first_name: Option<Secret<String>>,
    pub last_name: Option<Secret<String>>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PhoneDetails {
    pub number: Option<Secret<String>>,
    pub country_code: Option<String>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, serde::Deserialize)]
pub(crate) struct PaymentsCaptureRequest {
    pub payment_id: Option<String>,
    pub merchant_id: Option<String>,
    pub amount_to_capture: Option<i32>,
    pub refund_uncaptured_amount: Option<bool>,
    pub statement_descriptor_suffix: Option<String>,
    pub statement_descriptor_prefix: Option<String>,
}

#[derive(Default, Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct UrlDetails {
    pub url: String,
    pub method: String,
}
#[derive(Default, Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AuthenticationForStartResponse {
    pub authentication: UrlDetails,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextActionType {
    RedirectToUrl,
    DisplayQrCode,
    InvokeSdkClient,
    TriggerApi,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct NextAction {
    #[serde(rename = "type")]
    pub next_action_type: NextActionType,
    pub redirect_to_url: Option<String>,
}

#[derive(Setter, Clone, Default, Debug, Eq, PartialEq, serde::Serialize)]
pub struct PaymentsResponse {
    pub payment_id: Option<String>,
    pub merchant_id: Option<String>,
    pub status: enums::IntentStatus,
    pub amount: i32,
    pub amount_capturable: Option<i32>,
    pub amount_received: Option<i32>,
    pub client_secret: Option<Secret<String>>,
    #[serde(with = "custom_serde::iso8601::option")]
    pub created: Option<PrimitiveDateTime>,
    pub currency: String,
    pub customer_id: Option<String>,
    pub description: Option<String>,
    pub refunds: Option<Vec<RefundResponse>>,
    pub mandate_id: Option<String>,
    pub mandate_data: Option<MandateData>,
    pub setup_future_usage: Option<enums::FutureUsage>,
    pub off_session: Option<bool>,
    #[serde(with = "custom_serde::iso8601::option")]
    pub capture_on: Option<PrimitiveDateTime>,
    pub capture_method: Option<enums::CaptureMethod>,
    #[auth_based]
    pub payment_method: Option<enums::PaymentMethodType>,
    #[auth_based]
    pub payment_method_data: Option<PaymentMethodDataResponse>,
    pub payment_token: Option<i32>,
    pub shipping: Option<Address>,
    pub billing: Option<Address>,
    pub metadata: Option<serde_json::Value>,
    pub email: Option<Secret<String, pii::Email>>,
    pub name: Option<Secret<String>>,
    pub phone: Option<Secret<String>>,
    pub return_url: Option<String>,
    pub authentication_type: Option<enums::AuthenticationType>,
    pub statement_descriptor_name: Option<String>,
    pub statement_descriptor_suffix: Option<String>,
    pub next_action: Option<NextAction>,
    pub cancellation_reason: Option<String>,
    pub error_code: Option<String>, //TODO: Add error code column to the database
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaymentListConstraints {
    pub customer_id: Option<String>,
    pub starting_after: Option<String>,
    pub ending_before: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default, with = "custom_serde::iso8601::option")]
    pub created: Option<PrimitiveDateTime>,
    #[serde(default, with = "custom_serde::iso8601::option")]
    #[serde(rename = "created.lt")]
    pub created_lt: Option<PrimitiveDateTime>,
    #[serde(default, with = "custom_serde::iso8601::option")]
    #[serde(rename = "created.gt")]
    pub created_gt: Option<PrimitiveDateTime>,
    #[serde(default, with = "custom_serde::iso8601::option")]
    #[serde(rename = "created.lte")]
    pub created_lte: Option<PrimitiveDateTime>,
    #[serde(default, with = "custom_serde::iso8601::option")]
    #[serde(rename = "created.gte")]
    pub created_gte: Option<PrimitiveDateTime>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct PaymentListResponse {
    pub size: usize,
    pub data: Vec<PaymentsResponse>,
}

fn default_limit() -> i64 {
    10
}

#[derive(Default, Debug, serde::Deserialize, serde::Serialize)]
pub struct PaymentsRedirectionResponse {
    pub redirect_url: String,
}

impl PaymentsRedirectionResponse {
    pub fn new(redirect_url: &str) -> Self {
        Self {
            redirect_url: redirect_url.to_owned(),
        }
    }
}

impl From<PaymentsRequest> for PaymentsResponse {
    fn from(item: PaymentsRequest) -> Self {
        let payment_id = match item.payment_id {
            Some(api_types::PaymentIdType::PaymentIntentId(id)) => Some(id),
            _ => None,
        };

        Self {
            payment_id,
            merchant_id: item.merchant_id,
            setup_future_usage: item.setup_future_usage,
            off_session: item.off_session,
            shipping: item.shipping,
            billing: item.billing,
            metadata: item.metadata,
            capture_method: item.capture_method,
            payment_method: item.payment_method,
            capture_on: item.capture_on,
            payment_method_data: item
                .payment_method_data
                .map(PaymentMethodDataResponse::from),
            email: item.email,
            name: item.name,
            phone: item.phone,
            payment_token: item.payment_token,
            return_url: item.return_url,
            authentication_type: item.authentication_type,
            statement_descriptor_name: item.statement_descriptor_name,
            statement_descriptor_suffix: item.statement_descriptor_suffix,
            mandate_data: item.mandate_data,
            ..Default::default()
        }
    }
}

impl From<PaymentsStartRequest> for PaymentsResponse {
    fn from(item: PaymentsStartRequest) -> Self {
        Self {
            payment_id: Some(item.payment_id),
            merchant_id: Some(item.merchant_id),
            ..Default::default()
        }
    }
}

impl From<types::storage::PaymentIntent> for PaymentsResponse {
    fn from(item: types::storage::PaymentIntent) -> Self {
        Self {
            payment_id: Some(item.payment_id),
            merchant_id: Some(item.merchant_id),
            status: item.status,
            amount: item.amount,
            amount_capturable: item.amount_captured,
            client_secret: item.client_secret.map(|s| s.into()),
            created: Some(item.created_at),
            currency: item.currency.map(|c| c.to_string()).unwrap_or_default(),
            description: item.description,
            metadata: item.metadata,
            customer_id: item.customer_id,
            ..Self::default()
        }
    }
}

impl From<PaymentsStartRequest> for PaymentsRequest {
    fn from(item: PaymentsStartRequest) -> Self {
        Self {
            payment_id: Some(PaymentIdType::PaymentIntentId(item.payment_id)),
            merchant_id: Some(item.merchant_id),
            ..Default::default()
        }
    }
}

impl From<PaymentsRetrieveRequest> for PaymentsResponse {
    // After removing the request from the payments_to_payments_response this will no longer be needed
    fn from(item: PaymentsRetrieveRequest) -> Self {
        let payment_id = match item.resource_id {
            PaymentIdType::PaymentIntentId(id) => Some(id),
            _ => None,
        };

        Self {
            payment_id,
            merchant_id: item.merchant_id,
            ..Default::default()
        }
    }
}

impl From<PaymentsCancelRequest> for PaymentsResponse {
    fn from(item: PaymentsCancelRequest) -> Self {
        Self {
            payment_id: Some(item.payment_id),
            cancellation_reason: item.cancellation_reason,
            ..Default::default()
        }
    }
}

impl From<PaymentsCaptureRequest> for PaymentsResponse {
    // After removing the request from the payments_to_payments_response this will no longer be needed
    fn from(item: PaymentsCaptureRequest) -> Self {
        Self {
            payment_id: item.payment_id,
            amount_received: item.amount_to_capture,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PgRedirectResponse {
    pub payment_id: String,
    pub status: storage::enums::IntentStatus,
    pub gateway_id: String,
    pub customer_id: Option<String>,
    pub amount: Option<i32>,
}

#[derive(Debug, serde::Serialize, PartialEq, Eq, serde::Deserialize)]
pub struct RedirectionResponse {
    pub return_url: String,
    pub params: Vec<(String, String)>,
    pub return_url_with_query_params: String,
    pub http_method: api::Method,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, serde::Deserialize)]
pub struct PaymentsResponseForm {
    pub transaction_id: String,
    // pub transaction_reference_id: String,
    pub merchant_id: String,
    pub order_id: String,
}

// Extract only the last 4 digits of card
impl From<CCard> for CCardResponse {
    fn from(card: CCard) -> Self {
        let card_number_length = card.card_number.peek().clone().len();
        Self {
            last4: card.card_number.peek().clone()[card_number_length - 4..card_number_length]
                .to_string(),
            exp_month: card.card_exp_month.peek().clone(),
            exp_year: card.card_exp_year.peek().clone(),
        }
    }
}

impl From<PaymentMethod> for PaymentMethodDataResponse {
    fn from(payment_method_data: PaymentMethod) -> Self {
        match payment_method_data {
            PaymentMethod::Card(card) => PaymentMethodDataResponse::Card(CCardResponse::from(card)),
            PaymentMethod::BankTransfer => PaymentMethodDataResponse::BankTransfer,
            PaymentMethod::PayLater(pay_later_data) => {
                PaymentMethodDataResponse::PayLater(pay_later_data)
            }
            PaymentMethod::Wallet => PaymentMethodDataResponse::Wallet,
            PaymentMethod::Paypal => PaymentMethodDataResponse::Paypal,
        }
    }
}

impl From<enums::AttemptStatus> for enums::IntentStatus {
    fn from(s: enums::AttemptStatus) -> Self {
        match s {
            enums::AttemptStatus::Charged | enums::AttemptStatus::AutoRefunded => {
                enums::IntentStatus::Succeeded
            }

            enums::AttemptStatus::ConfirmationAwaited => enums::IntentStatus::RequiresConfirmation,
            enums::AttemptStatus::PaymentMethodAwaited => {
                enums::IntentStatus::RequiresPaymentMethod
            }

            enums::AttemptStatus::Authorized => enums::IntentStatus::RequiresCapture,
            enums::AttemptStatus::PendingVbv => enums::IntentStatus::RequiresCustomerAction,

            enums::AttemptStatus::PartialCharged
            | enums::AttemptStatus::Started
            | enums::AttemptStatus::VbvSuccessful
            | enums::AttemptStatus::Authorizing
            | enums::AttemptStatus::CodInitiated
            | enums::AttemptStatus::VoidInitiated
            | enums::AttemptStatus::CaptureInitiated
            | enums::AttemptStatus::Pending => enums::IntentStatus::Processing,

            enums::AttemptStatus::AuthenticationFailed
            | enums::AttemptStatus::AuthorizationFailed
            | enums::AttemptStatus::VoidFailed
            | enums::AttemptStatus::JuspayDeclined
            | enums::AttemptStatus::CaptureFailed
            | enums::AttemptStatus::Failure => enums::IntentStatus::Failed,
            enums::AttemptStatus::Voided => enums::IntentStatus::Cancelled,
        }
    }
}

pub trait PaymentAuthorize:
    api::ConnectorIntegration<Authorize, types::PaymentsRequestData, types::PaymentsResponseData>
{
}

pub trait PaymentSync:
    api::ConnectorIntegration<PSync, types::PaymentsRequestSyncData, types::PaymentsResponseData>
{
}

pub trait PaymentVoid:
    api::ConnectorIntegration<Void, types::PaymentRequestCancelData, types::PaymentsResponseData>
{
}

pub trait PaymentCapture:
    api::ConnectorIntegration<PCapture, types::PaymentsRequestCaptureData, types::PaymentsResponseData>
{
}

pub trait Payment:
    ConnectorCommon + PaymentAuthorize + PaymentSync + PaymentCapture + PaymentVoid
{
}
#[derive(Default, Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct PaymentsRetrieveRequest {
    pub resource_id: PaymentIdType,
    pub merchant_id: Option<String>,
    pub force_sync: bool,
    pub param: Option<String>,
    pub connector: Option<String>,
}

#[derive(Default, Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct PaymentRetrieveBody {
    pub merchant_id: Option<String>,
    pub force_sync: Option<bool>,
}
#[derive(Default, Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct PaymentsCancelRequest {
    #[serde(skip)]
    pub payment_id: String,
    pub cancellation_reason: Option<String>,
}

#[derive(Default, Debug, serde::Deserialize, serde::Serialize)]
pub struct PaymentsStartRequest {
    pub payment_id: String,
    pub merchant_id: String,
    pub txn_id: String,
}

#[cfg(test)]
mod payments_test {
    #![allow(clippy::expect_used)]

    use super::*;

    #[allow(dead_code)]
    fn card() -> CCard {
        CCard {
            card_number: "1234432112344321".to_string().into(),
            card_exp_month: "12".to_string().into(),
            card_exp_year: "99".to_string().into(),
            card_holder_name: "JohnDoe".to_string().into(),
            card_cvc: "123".to_string().into(),
        }
    }

    #[allow(dead_code)]
    fn payments_request() -> PaymentsRequest {
        PaymentsRequest {
            amount: Some(200),
            payment_method_data: Some(PaymentMethod::Card(card())),
            ..PaymentsRequest::default()
        }
    }

    //#[test] // FIXME: Fix test
    #[allow(dead_code)]
    fn verify_payments_request() {
        let pay_req = payments_request();
        let serialized =
            serde_json::to_string(&pay_req).expect("error serializing payments request");
        let _deserialized_pay_req: PaymentsRequest =
            serde_json::from_str(&serialized).expect("error de-serializing payments response");
        //assert_eq!(pay_req, deserialized_pay_req)
    }

    // Intended to test the serialization and deserialization of the enum PaymentIdType
    #[test]
    fn test_connector_id_type() {
        let sample_1 = PaymentIdType::PaymentIntentId("test_234565430uolsjdnf48i0".to_string());
        let s_sample_1 = serde_json::to_string(&sample_1).unwrap();
        let ds_sample_1 = serde_json::from_str::<PaymentIdType>(&s_sample_1).unwrap();
        assert_eq!(ds_sample_1, sample_1)
    }
}
