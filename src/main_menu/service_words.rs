//! What the menu says of a service and of an address typed for one: plain words, each with what
//! to do about it. No error text of the system is shown: it goes to the log.

use bota_radovid::{AddressFault, ClientError, PROTO_VERSION, ServiceFault};

use super::services::Verdict;

/// Said when the list is full.
pub const TOO_MANY: &str = "The list is full. Remove a service before adding another.";
/// Said when sign-in is pressed before the service passed its check.
pub const NOT_CHECKED: &str = "Wait until the service has been checked.";
/// Said of a service that is built in or only given for this run.
pub const CANNOT_REMOVE: &str = "Only a service you added can be removed.";

/// How a verdict looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Not checked yet, and nothing under way.
    Idle,
    Waiting,
    Good,
    Bad,
}

/// What a verdict says: the finding, and what to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Words {
    pub tone: Tone,
    pub headline: String,
    pub advice: String,
}

/// The words of the check of a service; `None` is a service not checked (since it was listed,
/// or since its check was dropped when the player moved on).
pub fn verdict(verdict: Option<&Verdict>) -> Words {
    match verdict {
        None => Words {
            tone: Tone::Idle,
            headline: "Not checked yet".to_owned(),
            advice: "Check it again to see whether it works.".to_owned(),
        },
        Some(Verdict::Checking) => Words {
            tone: Tone::Waiting,
            headline: "Checking the service…".to_owned(),
            advice: String::new(),
        },
        Some(Verdict::Works(found)) => Words {
            tone: Tone::Good,
            headline: "Works".to_owned(),
            advice: if found.name.is_empty() {
                "You can sign in.".to_owned()
            } else {
                format!("It calls itself {}. You can sign in.", found.name)
            },
        },
        Some(Verdict::Failed(fault)) => {
            let (headline, advice) = fault_words(*fault);
            Words {
                tone: Tone::Bad,
                headline,
                advice,
            }
        }
    }
}

fn fault_words(fault: ServiceFault) -> (String, String) {
    let (headline, advice) = match fault {
        ServiceFault::BadAddress => (
            "This is not a service address",
            "Use a host name, like bota.lubush.space.".to_owned(),
        ),
        ServiceFault::HostNotFound => (
            "Cannot find the host",
            "Check the spelling of the address and your internet connection.".to_owned(),
        ),
        ServiceFault::Refused => (
            "Nobody is listening at this address",
            "Check the port. If the address is right, the service is down: try again later."
                .to_owned(),
        ),
        ServiceFault::TimedOut => (
            "Nobody answered in time",
            "Check the address and your internet connection, or try again later.".to_owned(),
        ),
        ServiceFault::Insecure => (
            "The connection is not secure",
            "The service's certificate is not valid for this address. Check the address, or ask \
             the service's owner."
                .to_owned(),
        ),
        ServiceFault::NotService => (
            "Something answers, but it is not this kind of service",
            "Check the whole address, including the part after the host name.".to_owned(),
        ),
        ServiceFault::Hangup => (
            "The service hung up without answering",
            "It may be restarting. Check again in a minute.".to_owned(),
        ),
        ServiceFault::Newer { server } => (
            "The service is newer than this client",
            format!(
                "Update the client to use it. (Service: version {server}, client: {PROTO_VERSION}.)"
            ),
        ),
        ServiceFault::Older { server } => (
            "The service is older than this client",
            format!(
                "Ask its owner to update it, or choose another service. (Service: version {server}, client: {PROTO_VERSION}.)"
            ),
        ),
        ServiceFault::Incompatible => (
            "The service speaks another version of the protocol",
            "Update the client, or choose another service.".to_owned(),
        ),
        ServiceFault::Busy => ("The service is busy", "Check again in a minute.".to_owned()),
        ServiceFault::TooManyChecks => (
            "Earlier checks are still running",
            "Check again in a moment.".to_owned(),
        ),
        ServiceFault::CannotConnect => (
            "The browser could not connect",
            "The address may be wrong, the service may be down, or its certificate may not be \
             valid. Check the address and try again."
                .to_owned(),
        ),
    };
    (headline.to_owned(), advice)
}

/// What a lost or failed connection to a service says: no error text of the system, which goes to
/// the log.
pub fn connection_words(error: &ClientError) -> String {
    match error {
        ClientError::Io(_) => {
            bevy::log::warn!("connection to the service failed: {error}");
            "check your internet connection, or try again in a minute".to_owned()
        }
        ClientError::BadUrl(_) => {
            bevy::log::warn!("the service address cannot be used: {error}");
            "its address is not valid: choose the service again".to_owned()
        }
        ClientError::Protocol(_) => {
            bevy::log::warn!("the service sent what this client cannot read: {error}");
            "it sent something this client cannot read: update the client".to_owned()
        }
        ClientError::Refused(_) | ClientError::Closed | ClientError::Timeout => error.to_string(),
    }
}

/// The command line's service: its canonical address, or why it is none.
pub fn service_argument(typed: &str) -> Result<String, String> {
    bota_radovid::service_address(typed).map_err(|fault| address_fault(fault).to_owned())
}

/// Why an address typed for a service is none, and what to change.
pub fn address_fault(fault: AddressFault) -> &'static str {
    match fault {
        AddressFault::Empty => "Type the service's address, like bota.lubush.space.",
        AddressFault::TooLong => "The address is too long.",
        AddressFault::Spaces => "The address has a space in it. Remove it.",
        AddressFault::Characters => "The address has a character that cannot be in an address.",
        AddressFault::Scheme => {
            "A service address starts with wss:// or has no start at all, like bota.lubush.space."
        }
        AddressFault::Plain => {
            "A service is reached over a secure connection. Use wss://, or leave the start out."
        }
        AddressFault::Credentials => "The address has a login in it (before @). Remove that part.",
        AddressFault::QueryOrFragment => "Remove everything from ? or # onward.",
        AddressFault::Host => {
            "The host name is not valid. Use letters, digits, dashes and dots, like bota.lubush.space."
        }
        AddressFault::Port => "The port after the colon must be a number from 1 to 65535.",
        AddressFault::Path => "The part after the host name is not valid.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAULTS: [ServiceFault; 13] = [
        ServiceFault::BadAddress,
        ServiceFault::HostNotFound,
        ServiceFault::Refused,
        ServiceFault::TimedOut,
        ServiceFault::Insecure,
        ServiceFault::NotService,
        ServiceFault::Hangup,
        ServiceFault::Newer { server: 99 },
        ServiceFault::Older { server: 1 },
        ServiceFault::Incompatible,
        ServiceFault::Busy,
        ServiceFault::TooManyChecks,
        ServiceFault::CannotConnect,
    ];

    #[test]
    fn every_outcome_of_a_check_has_its_own_finding_and_something_to_do() {
        let mut headlines = std::collections::HashSet::new();
        for fault in FAULTS {
            let words = verdict(Some(&Verdict::Failed(fault)));
            assert_eq!(words.tone, Tone::Bad);
            assert!(
                !words.headline.is_empty() && !words.advice.is_empty(),
                "{fault:?}"
            );
            for text in [&words.headline, &words.advice] {
                assert!(
                    !text.to_lowercase().contains("os error")
                        && !text.contains("::")
                        && !text.contains("Os {"),
                    "{fault:?}: {text}"
                );
            }
            // Newer and older share a finding's shape but not its words.
            headlines.insert(words.headline);
        }
        assert_eq!(headlines.len(), FAULTS.len(), "findings told apart");
    }

    #[test]
    fn a_service_that_works_says_so_with_the_name_it_gave() {
        let found = |name: &str| {
            Verdict::Works(bota_radovid::ServiceFound {
                name: name.to_owned(),
            })
        };
        let named = verdict(Some(&found("Arena")));
        assert_eq!(named.tone, Tone::Good);
        assert_eq!(named.headline, "Works");
        assert!(named.advice.contains("Arena"));
        assert_eq!(verdict(Some(&found(""))).advice, "You can sign in.");
        assert_eq!(verdict(None).tone, Tone::Idle);
        assert_eq!(verdict(Some(&Verdict::Checking)).tone, Tone::Waiting);
    }

    #[test]
    fn every_reason_an_address_is_refused_names_what_to_change() {
        for fault in [
            AddressFault::Empty,
            AddressFault::TooLong,
            AddressFault::Spaces,
            AddressFault::Characters,
            AddressFault::Scheme,
            AddressFault::Plain,
            AddressFault::Credentials,
            AddressFault::QueryOrFragment,
            AddressFault::Host,
            AddressFault::Port,
            AddressFault::Path,
        ] {
            assert!(address_fault(fault).ends_with('.'), "{fault:?}");
        }
    }
}
