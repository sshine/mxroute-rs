//! Values the API constrains, modelled so the constraint is not a comment.
//!
//! Each of these exists because the wire form is easy to get wrong: a magic zero that
//! means "no limit", two magic strings among otherwise ordinary email addresses, and an
//! integer with a documented ceiling a caller would otherwise discover by being rejected.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::InvalidValue;

/// Largest daily send limit the API accepts.
pub const MAX_SEND_LIMIT: u32 = 9600;

/// A mailbox's disk allowance.
///
/// The wire form is an integer count of megabytes in which `0` means unlimited, so an
/// arithmetic comparison against a plain number gets the unlimited case backwards.
///
/// ```
/// use mxroute::MailboxQuota;
///
/// assert_eq!(MailboxQuota::from_wire(0), MailboxQuota::Unlimited);
/// assert_eq!(MailboxQuota::from_wire(1024), MailboxQuota::Megabytes(1024));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MailboxQuota {
    /// No limit. Sent and received as `0`.
    Unlimited,
    /// A limit in megabytes.
    Megabytes(u32),
}

impl MailboxQuota {
    /// Reads the wire form, where `0` is unlimited.
    pub fn from_wire(megabytes: u32) -> Self {
        match megabytes {
            0 => Self::Unlimited,
            mb => Self::Megabytes(mb),
        }
    }

    /// The wire form, where unlimited is `0`.
    pub fn to_wire(self) -> u32 {
        match self {
            Self::Unlimited => 0,
            Self::Megabytes(mb) => mb,
        }
    }

    /// The limit in megabytes, or `None` when there is none.
    pub fn megabytes(self) -> Option<u32> {
        match self {
            Self::Unlimited => None,
            Self::Megabytes(mb) => Some(mb),
        }
    }
}

impl fmt::Display for MailboxQuota {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlimited => f.write_str("unlimited"),
            Self::Megabytes(mb) => write!(f, "{mb} MB"),
        }
    }
}

impl Serialize for MailboxQuota {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_wire().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MailboxQuota {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u32::deserialize(deserializer).map(Self::from_wire)
    }
}

/// A mailbox's daily send cap.
///
/// The API documents a maximum of [`MAX_SEND_LIMIT`]; [`new`](SendLimit::new) refuses more
/// rather than spending a request to be told so.
///
/// Deserialization trusts whatever the server reports, so raising the cap upstream cannot
/// make existing mailboxes undecodable. Only values this crate is about to send are
/// checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SendLimit(u32);

impl SendLimit {
    /// Builds a send limit, rejecting more than [`MAX_SEND_LIMIT`].
    pub fn new(messages_per_day: u32) -> Result<Self, InvalidValue> {
        if messages_per_day > MAX_SEND_LIMIT {
            return Err(InvalidValue::new(
                "limit",
                "must not exceed 9600 messages a day",
                messages_per_day.to_string(),
            ));
        }
        Ok(Self(messages_per_day))
    }

    /// The largest limit the API accepts.
    pub fn max() -> Self {
        Self(MAX_SEND_LIMIT)
    }

    /// The number of messages a day.
    pub fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for SendLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<u32> for SendLimit {
    type Error = InvalidValue;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Where a forwarder sends mail.
///
/// Two destinations are not addresses at all: the API reads `:blackhole:` as "accept and
/// discard" and `:fail:` as "reject at delivery". Left as strings they are indistinguishable
/// from a typo, and quietly discarding mail is not a failure anyone notices.
///
/// ```
/// use mxroute::Destination;
///
/// assert_eq!(
///     Destination::from(":blackhole:".to_owned()),
///     Destination::Blackhole,
/// );
/// assert_eq!(Destination::Fail.as_wire(), ":fail:");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Destination {
    /// An ordinary recipient address.
    Address(String),
    /// Accept the message and discard it, sent as `:blackhole:`.
    Blackhole,
    /// Refuse the message at delivery, sent as `:fail:`.
    Fail,
}

impl Destination {
    /// The wire form.
    pub fn as_wire(&self) -> &str {
        match self {
            Self::Address(address) => address,
            Self::Blackhole => ":blackhole:",
            Self::Fail => ":fail:",
        }
    }

    /// An ordinary address destination.
    pub fn address(address: impl Into<String>) -> Self {
        Self::from(address.into())
    }
}

impl From<String> for Destination {
    /// Recognizes the two magic values; anything else is an address.
    fn from(value: String) -> Self {
        match value.as_str() {
            ":blackhole:" => Self::Blackhole,
            ":fail:" => Self::Fail,
            _ => Self::Address(value),
        }
    }
}

impl From<&str> for Destination {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}

impl fmt::Display for Destination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

impl Serialize for Destination {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for Destination {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_zero_quota_is_unlimited_in_both_directions() {
        assert_eq!(MailboxQuota::from_wire(0), MailboxQuota::Unlimited);
        assert_eq!(MailboxQuota::Unlimited.to_wire(), 0);
        assert_eq!(MailboxQuota::Unlimited.megabytes(), None);
        assert_eq!(MailboxQuota::Megabytes(1024).megabytes(), Some(1024));
    }

    #[test]
    fn a_quota_round_trips_through_its_integer_form() {
        for quota in [
            MailboxQuota::Unlimited,
            MailboxQuota::Megabytes(1),
            MailboxQuota::Megabytes(1024),
        ] {
            let json = serde_json::to_string(&quota).expect("serializes");
            let back: MailboxQuota = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(back, quota);
        }
        assert_eq!(
            serde_json::to_string(&MailboxQuota::Unlimited).expect("serializes"),
            "0"
        );
    }

    #[test]
    fn unlimited_sorts_below_every_finite_quota() {
        // The wire form would sort it the other way round, since 0 is the smallest
        // integer. The enum ordering says what the value means.
        assert!(MailboxQuota::Unlimited < MailboxQuota::Megabytes(1));
    }

    #[test]
    fn a_send_limit_over_the_documented_cap_is_refused() {
        assert!(SendLimit::new(9601).is_err());
        assert_eq!(
            SendLimit::new(9600).expect("the cap itself is fine").get(),
            9600
        );
        assert_eq!(SendLimit::max().get(), MAX_SEND_LIMIT);
        assert!(SendLimit::try_from(0).is_ok());
    }

    #[test]
    fn a_send_limit_the_server_reports_is_taken_as_given() {
        // Raising the cap upstream must not make existing mailboxes undecodable.
        let limit: SendLimit = serde_json::from_str("50000").expect("reads trust the server");
        assert_eq!(limit.get(), 50_000);
        assert_eq!(
            serde_json::to_string(&SendLimit::max()).expect("serializes"),
            "9600"
        );
    }
    #[test]
    fn the_magic_destinations_are_recognized_rather_than_left_as_addresses() {
        assert_eq!(Destination::from(":blackhole:"), Destination::Blackhole);
        assert_eq!(Destination::from(":fail:"), Destination::Fail);
        assert_eq!(
            Destination::from("someone@example.com"),
            Destination::Address("someone@example.com".to_owned())
        );
        // Including through the constructor a caller reaches for by name.
        assert_eq!(Destination::address(":fail:"), Destination::Fail);
    }

    #[test]
    fn a_destination_round_trips_as_a_bare_string() {
        for destination in [
            Destination::Blackhole,
            Destination::Fail,
            Destination::address("someone@example.com"),
        ] {
            let json = serde_json::to_string(&destination).expect("serializes");
            let back: Destination = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(back, destination);
        }
        assert_eq!(
            serde_json::to_string(&Destination::Blackhole).expect("serializes"),
            r#"":blackhole:""#
        );
    }
}
