//! The services a player can sign in to: the ones built in, the page's own (in a browser) and
//! the ones the player added; the one chosen; and the check of each before anyone signs in to it.
//! A service passes the check by answering the protocol's own first exchange, so that a wrong
//! address, a service that is down and a server that is something else are told when the address
//! is entered and not as an unclear error at sign-in.

use std::time::Duration;

use bota_radovid::{
    ServiceCheck, ServiceFault, ServiceFound, Tls, same_matchmaker, service_address,
};

use super::core::{Act, MenuCore, Page};
use super::prefs::{MAX_ADDED_SERVICES, Prefs};
use super::service_words;

/// The host of the public service every client offers. The one place that says where it is.
pub const BUILT_IN_HOST: &str = "bota.lubush.space";
/// What the built-in service is called.
pub const BUILT_IN_NAME: &str = "Public service";
/// What the service of the page a browser runs in is called.
pub const PAGE_NAME: &str = "This page's service";
/// How long a check waits for the service before it gives it up.
pub const CHECK_PATIENCE: Duration = Duration::from_secs(8);
/// Checks kept at most: every service that can be listed, and the one chosen.
const MAX_CHECKS: usize = MAX_ADDED_SERVICES + 3;

#[cfg(test)]
thread_local! {
    /// A test that runs the menu as it runs for a new player names the built-in service here
    /// (a real service of the test's own); no other test ever looks the public one up.
    static TEST_BUILT_IN: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The address of the built-in service: the public one, or a test's.
pub fn built_in_address() -> String {
    #[cfg(test)]
    return TEST_BUILT_IN.with(|built_in| {
        built_in
            .borrow()
            .clone()
            .unwrap_or_else(|| "wss://built-in.invalid".to_owned())
    });
    #[cfg(not(test))]
    format!("wss://{BUILT_IN_HOST}")
}

/// Tests that do not run the menu as a new player's start with no chosen service.
fn first_choice() -> String {
    #[cfg(test)]
    if TEST_BUILT_IN.with(|built_in| built_in.borrow().is_none()) {
        return String::new();
    }
    built_in_address()
}

/// Makes the menus built on this thread run as a new player's, with `address` as the built-in
/// service, until the guard is dropped.
#[cfg(test)]
pub(crate) fn built_in_for_test(address: &str) -> BuiltInGuard {
    TEST_BUILT_IN.with(|built_in| *built_in.borrow_mut() = Some(address.to_owned()));
    BuiltInGuard
}

#[cfg(test)]
pub(crate) struct BuiltInGuard;

#[cfg(test)]
impl Drop for BuiltInGuard {
    fn drop(&mut self) {
        TEST_BUILT_IN.with(|built_in| *built_in.borrow_mut() = None);
    }
}

/// Where a listed service comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The public service of this client.
    BuiltIn,
    /// The service of the page this browser client was loaded from.
    Page,
    /// Added by the player: the only ones that can be removed.
    Added,
    /// Named for this run (`--service`), not kept.
    Given,
}

/// A service in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// The canonical address.
    pub address: String,
    pub origin: Origin,
}

impl Entry {
    pub fn removable(&self) -> bool {
        self.origin == Origin::Added
    }
}

/// What a service's check says now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Checking,
    Works(ServiceFound),
    Failed(ServiceFault),
}

/// What the player does with a service of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceAct {
    /// Signs in to this one from now on.
    Use(String),
    /// Checks it again.
    Recheck(String),
    /// Takes an added one off the list.
    Remove(String),
}

/// The services as listed: the built-in one, the page's, the added ones, and the one chosen if it
/// is none of them.
pub fn entries(prefs: &Prefs, page: Option<&str>) -> Vec<Entry> {
    let mut list = vec![Entry {
        name: BUILT_IN_NAME.to_owned(),
        address: built_in_address(),
        origin: Origin::BuiltIn,
    }];
    let listed = |list: &[Entry], address: &str| list.iter().any(|e| same(&e.address, address));
    if let Some(page) = page.filter(|page| !listed(&list, page)) {
        list.push(Entry {
            name: PAGE_NAME.to_owned(),
            address: page.to_owned(),
            origin: Origin::Page,
        });
    }
    for address in &prefs.added_services {
        if !listed(&list, address) {
            list.push(Entry {
                name: host_name(address),
                address: address.clone(),
                origin: Origin::Added,
            });
        }
    }
    if !prefs.service.is_empty() && !listed(&list, &prefs.service) {
        list.push(Entry {
            name: host_name(&prefs.service),
            address: prefs.service.clone(),
            origin: Origin::Given,
        });
    }
    list
}

fn same(left: &str, right: &str) -> bool {
    left == right || same_matchmaker(left, right)
}

/// What a service without a name of its own is called: its address without the scheme.
fn host_name(address: &str) -> String {
    address.strip_prefix("wss://").unwrap_or(address).to_owned()
}

/// The checks, one per service at most, and what each found.
#[derive(Default)]
pub struct Checks {
    slots: Vec<Slot>,
}

struct Slot {
    address: String,
    running: Option<ServiceCheck>,
    verdict: Verdict,
}

impl Checks {
    pub fn verdict(&self, address: &str) -> Option<&Verdict> {
        self.slot(address).map(|slot| &slot.verdict)
    }

    fn slot(&self, address: &str) -> Option<&Slot> {
        self.slots.iter().find(|slot| same(&slot.address, address))
    }

    /// Starts checking `address` unless a check of it runs already: one at a time per service.
    /// Returns whether one was started.
    pub fn start(&mut self, address: &str, tls: &Tls, patience: Duration) -> bool {
        let at = match self
            .slots
            .iter()
            .position(|slot| same(&slot.address, address))
        {
            Some(at) => at,
            None => {
                if self.slots.len() >= MAX_CHECKS {
                    // A service that is no longer listed has its place taken.
                    self.slots.remove(0);
                }
                self.slots.push(Slot {
                    address: address.to_owned(),
                    running: None,
                    verdict: Verdict::Checking,
                });
                self.slots.len() - 1
            }
        };
        let slot = &mut self.slots[at];
        if slot.running.is_some() {
            return false;
        }
        slot.verdict = Verdict::Checking;
        slot.running = Some(ServiceCheck::start(address, tls, patience));
        true
    }

    /// Takes in what the running checks found; the addresses whose verdict changed.
    pub fn poll(&mut self) -> Vec<String> {
        let mut changed = Vec::new();
        for slot in &mut self.slots {
            let Some(running) = &mut slot.running else {
                continue;
            };
            let Some(outcome) = running.poll() else {
                continue;
            };
            slot.running = None;
            slot.verdict = match outcome {
                Ok(found) => {
                    bevy::log::info!("service {} passed its check", slot.address);
                    Verdict::Works(found)
                }
                Err(fault) => {
                    bevy::log::info!("service {} did not pass its check: {fault:?}", slot.address);
                    Verdict::Failed(fault)
                }
            };
            changed.push(slot.address.clone());
        }
        changed
    }

    /// Ends every check that runs but `keep`'s: the player has moved on from those services.
    pub fn cancel_running_except(&mut self, keep: &str) {
        self.slots
            .retain(|slot| slot.running.is_none() || same(&slot.address, keep));
    }

    /// Forgets the check of `address`, ending it if it runs.
    pub fn forget(&mut self, address: &str) {
        self.slots.retain(|slot| !same(&slot.address, address));
    }

    /// Checks running now.
    #[cfg(test)]
    pub fn running(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.running.is_some())
            .count()
    }
}

impl MenuCore {
    /// The service the player signs in to, if one is chosen.
    pub(super) fn chosen_service(&self) -> Option<&str> {
        Some(self.prefs.service.as_str()).filter(|service| !service.is_empty())
    }

    /// The services as listed.
    pub(super) fn service_entries(&self) -> Vec<Entry> {
        entries(&self.prefs, self.page_service.as_deref())
    }

    /// The check of the chosen service.
    pub(super) fn chosen_verdict(&self) -> Option<&Verdict> {
        self.checks.verdict(self.chosen_service()?)
    }

    /// Whether sign-in and account creation are open: the service passed its check (an account
    /// already signed in has been past it).
    pub fn service_ready(&self) -> bool {
        self.account || matches!(self.chosen_verdict(), Some(Verdict::Works(_)))
    }

    /// A service an older settings file remembered, unlisted, is one the player added.
    pub(super) fn adopt_remembered_service(prefs: &mut Prefs, page: Option<&str>) {
        let unlisted = entries(prefs, page)
            .iter()
            .any(|entry| entry.origin == Origin::Given);
        if unlisted && prefs.added_services.len() < MAX_ADDED_SERVICES {
            prefs.added_services.push(prefs.service.clone());
        }
    }

    /// The first choice of a menu that has none yet is the built-in service.
    pub(super) fn choose_first_service(prefs: &mut Prefs) {
        if prefs.service.is_empty() {
            prefs.service = first_choice();
        }
    }

    /// Takes in the checks' findings and starts the chosen service's check when it has none.
    pub(super) fn drive_service_checks(&mut self) {
        // Under a game named on the command line nothing is looked up until the game is left.
        if self.page_beneath() != Page::Game
            && let Some(address) = self.chosen_service().map(str::to_owned)
            && self.checks.verdict(&address).is_none()
        {
            self.checks.start(&address, &self.tls, self.check_patience);
        }
        if !self.checks.poll().is_empty() {
            self.sync_service_controls();
        }
    }

    /// What the player does with a service of the list.
    pub(super) fn service_act(&mut self, act: ServiceAct) {
        match act {
            ServiceAct::Use(address) => self.use_service(address),
            ServiceAct::Recheck(address) => {
                self.checks.forget(&address);
                self.checks.start(&address, &self.tls, self.check_patience);
                self.sync_service_controls();
            }
            ServiceAct::Remove(address) => self.remove_service(&address),
        }
    }

    /// Signs in to `address` from now on: the sign-in that was open, the account and what was
    /// fetched from the other service are left, and the service is checked.
    pub(super) fn use_service(&mut self, address: String) {
        let on_the_list = self.page == Page::Service;
        if !same(&self.prefs.service, &address) {
            self.logout();
            self.drop_link();
        }
        self.prefs.service = address.clone();
        self.save_prefs();
        self.status = None;
        self.checks.cancel_running_except(&address);
        self.checks.forget(&address);
        self.checks.start(&address, &self.tls, self.check_patience);
        // `logout` opened the sign-in page; the list stays where the player is.
        if on_the_list {
            self.go_form(Page::Service);
        } else {
            self.sync_service_controls();
        }
    }

    /// Adds the address typed in the Service page's field, or says why it is none.
    pub(super) fn add_service(&mut self) {
        let typed = self.form.value(0).to_owned();
        let address = match service_address(&typed) {
            Ok(address) => address,
            Err(fault) => {
                self.tell(service_words::address_fault(fault), true);
                return;
            }
        };
        let known = self
            .service_entries()
            .into_iter()
            .find(|entry| same(&entry.address, &address));
        if let Some(entry) = known {
            self.form.put(0, "");
            self.use_service(entry.address);
            return;
        }
        if self.prefs.added_services.len() >= MAX_ADDED_SERVICES {
            self.tell(service_words::TOO_MANY.to_owned(), true);
            return;
        }
        self.prefs.added_services.push(address.clone());
        self.form.put(0, "");
        self.use_service(address);
    }

    fn remove_service(&mut self, address: &str) {
        let Some(at) = self
            .prefs
            .added_services
            .iter()
            .position(|added| same(added, address))
        else {
            self.tell(service_words::CANNOT_REMOVE.to_owned(), true);
            return;
        };
        let removed = self.prefs.added_services.remove(at);
        self.checks.forget(&removed);
        if same(&self.prefs.service, &removed) {
            self.use_service(first_choice());
        } else {
            self.save_prefs();
            self.sync_service_controls();
        }
    }

    /// The Service page's rows and the way in's form follow what the checks found.
    pub(super) fn sync_service_controls(&mut self) {
        // Off the list, only the chosen service's check is of any use.
        if self.page != Page::Service
            && let Some(chosen) = self.chosen_service().map(str::to_owned)
        {
            self.checks.cancel_running_except(&chosen);
        }
        let controls: Vec<Act> = match self.page {
            Page::Service => self
                .service_entries()
                .iter()
                .flat_map(|entry| self.row_acts(entry))
                .collect(),
            // A service that failed offers its check again in place of the form.
            Page::Login | Page::Register => match (self.chosen_service(), self.chosen_verdict()) {
                (Some(address), Some(Verdict::Failed(_))) => {
                    vec![Act::Service(ServiceAct::Recheck(address.to_owned()))]
                }
                _ => Vec::new(),
            },
            _ => return,
        };
        self.form
            .set_service_controls(self.service_ready(), controls);
    }

    fn row_acts(&self, entry: &Entry) -> Vec<Act> {
        let chosen = self
            .chosen_service()
            .is_some_and(|c| same(c, &entry.address));
        let checking = matches!(self.checks.verdict(&entry.address), Some(Verdict::Checking));
        let mut acts = Vec::new();
        if !chosen {
            acts.push(Act::Service(ServiceAct::Use(entry.address.clone())));
        }
        if !checking {
            acts.push(Act::Service(ServiceAct::Recheck(entry.address.clone())));
        }
        if entry.removable() {
            acts.push(Act::Service(ServiceAct::Remove(entry.address.clone())));
        }
        acts
    }

    /// The rows of the Service page.
    pub(super) fn service_rows(&self) -> Vec<super::view::ServiceRow> {
        self.service_entries()
            .into_iter()
            .map(|entry| {
                let verdict = self.checks.verdict(&entry.address).cloned();
                let words = service_words::verdict(verdict.as_ref());
                super::view::ServiceRow {
                    selected: self
                        .chosen_service()
                        .is_some_and(|c| same(c, &entry.address)),
                    removable: entry.removable(),
                    checking: matches!(verdict, Some(Verdict::Checking)),
                    name: entry.name,
                    address: entry.address,
                    words,
                }
            })
            .collect()
    }

    /// What the way in's card says of the chosen service.
    pub(super) fn service_banner(&self) -> Option<super::view::ServiceBanner> {
        let address = self.chosen_service()?;
        let entry = self
            .service_entries()
            .into_iter()
            .find(|entry| same(&entry.address, address))?;
        // Not started yet is about to be: the next frame starts it.
        let verdict = self.chosen_verdict().cloned().unwrap_or(Verdict::Checking);
        Some(super::view::ServiceBanner {
            checking: verdict == Verdict::Checking,
            words: service_words::verdict(Some(&verdict)),
            subject: match entry.origin {
                // Its own name says which it is; a service the player typed is known by its
                // host, which a name it gave itself could not stand in for.
                Origin::BuiltIn => entry.name.clone(),
                _ => host_name(&entry.address),
            },
            name: entry.name,
            address: entry.address,
            open: self.service_ready(),
        })
    }

    /// What the Service page says of the address being typed: where it would go, or why not.
    pub(super) fn typed_service(&self) -> super::view::TypedService {
        use super::view::TypedService;
        let typed = self.form.value(0);
        if typed.trim().is_empty() {
            return TypedService::Nothing;
        }
        match service_address(typed) {
            Ok(address) => TypedService::Address(address),
            Err(fault) => TypedService::Refused(service_words::address_fault(fault)),
        }
    }
}

#[cfg(test)]
impl MenuCore {
    /// Makes the checks of this menu give up after `patience`: spent already, for a test of the
    /// time-out that waits for no clock.
    pub(crate) fn set_check_patience_for_test(&mut self, patience: Duration) {
        self.check_patience = patience;
    }

    /// The verdict of `address`'s check.
    pub(crate) fn verdict_for_test(&self, address: &str) -> Option<Verdict> {
        self.checks.verdict(address).cloned()
    }

    /// The services as listed.
    pub(crate) fn services_for_test(&self) -> Vec<Entry> {
        self.service_entries()
    }

    /// The service signed in to.
    pub(crate) fn chosen_service_for_test(&self) -> Option<String> {
        self.chosen_service().map(str::to_owned)
    }

    /// What is typed in the Service page's field.
    pub(crate) fn typed_for_test(&self) -> String {
        self.form.value(0).to_owned()
    }

    /// Whether a check is still running for any service.
    pub(crate) fn checks_running_for_test(&self) -> usize {
        self.checks.running()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_service_is_the_public_one_in_the_canonical_form() {
        let address = format!("wss://{BUILT_IN_HOST}");
        assert_eq!(address, "wss://bota.lubush.space");
        assert_eq!(service_address(&address), Ok(address));
    }

    #[test]
    fn a_settings_file_of_before_the_list_reads_and_what_is_added_is_written_back() {
        let old = "service = Match.Example.org:4460/control\nlogin = alice\n";
        let prefs = Prefs::parse(old);
        assert_eq!(prefs.service, "wss://match.example.org:4460");
        assert_eq!(prefs.login, "alice");
        assert!(prefs.added_services.is_empty());
        let mut prefs = Prefs::parse(
            "service_added = https://one.example.org/page\nservice_added = two.example.org\n\
             service_added = wss://one.example.org\nservice_added = ws://nonsense\n",
        );
        assert_eq!(
            prefs.added_services,
            ["wss://one.example.org", "wss://two.example.org"],
            "canonical, once each, and what is no address is dropped"
        );
        prefs.service = "wss://two.example.org".to_owned();
        let again = Prefs::parse(&prefs.render());
        assert_eq!(again, prefs);
    }

    #[test]
    fn no_more_services_are_read_than_a_player_may_add() {
        let text: String = (0..MAX_ADDED_SERVICES + 5)
            .map(|n| format!("service_added = s{n}.example.org\n"))
            .collect();
        assert_eq!(Prefs::parse(&text).added_services.len(), MAX_ADDED_SERVICES);
    }

    #[test]
    fn the_list_has_the_built_in_service_first_and_the_one_signed_in_to_even_when_given() {
        let mut prefs = Prefs {
            added_services: vec!["wss://added.example.org".to_owned()],
            service: "wss://given.example.org".to_owned(),
            ..Prefs::default()
        };
        let list = entries(&prefs, Some("wss://page.example.org"));
        let origins: Vec<Origin> = list.iter().map(|entry| entry.origin).collect();
        assert_eq!(
            origins,
            [Origin::BuiltIn, Origin::Page, Origin::Added, Origin::Given]
        );
        assert!(
            list.iter()
                .all(|entry| entry.removable() == (entry.origin == Origin::Added))
        );
        // The page's own service that is also the built-in one is listed once.
        prefs.service = built_in_address();
        let list = entries(&prefs, Some(&built_in_address()));
        assert_eq!(list.len(), 2, "{list:?}");
    }
}
