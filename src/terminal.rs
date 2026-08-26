#[derive(Debug, PartialEq, Eq)]
pub(super) enum TerminalFailure<P, R> {
    Primary(P),
    Release(R),
}

pub(super) fn finalize_owned_terminal<T, P, R>(
    primary: Result<T, P>,
    complete_owned: impl FnOnce(Result<T, P>) -> Result<T, P>,
    release: impl FnOnce() -> Result<(), R>,
) -> Result<T, TerminalFailure<P, R>> {
    let completed = complete_owned(primary);
    match release() {
        Ok(()) => completed.map_err(TerminalFailure::Primary),
        Err(error) => Err(TerminalFailure::Release(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::{TerminalFailure, finalize_owned_terminal};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct FakeGuard {
        releases: Rc<Cell<usize>>,
        released: bool,
    }

    impl FakeGuard {
        fn release(mut self) {
            if !self.released {
                self.releases.set(self.releases.get() + 1);
                self.released = true;
            }
        }
    }

    impl Drop for FakeGuard {
        fn drop(&mut self) {
            if !self.released {
                self.releases.set(self.releases.get() + 1);
                self.released = true;
            }
        }
    }

    #[test]
    fn completion_precedes_exactly_one_release() {
        let events = RefCell::new(Vec::new());
        let releases = Cell::new(0);

        let result = finalize_owned_terminal(
            Ok::<_, &'static str>(7_u8),
            |primary| {
                events.borrow_mut().push("complete");
                primary
            },
            || {
                events.borrow_mut().push("release");
                releases.set(releases.get() + 1);
                Ok::<_, &'static str>(())
            },
        );

        assert_eq!(result, Ok(7));
        assert_eq!(&*events.borrow(), &["complete", "release"]);
        assert_eq!(releases.get(), 1);
    }

    #[test]
    fn primary_failure_survives_successful_release() {
        let result = finalize_owned_terminal(
            Err::<(), _>("workload"),
            |primary| primary,
            || Ok::<(), &'static str>(()),
        );

        assert_eq!(result, Err(TerminalFailure::Primary("workload")));
    }

    #[test]
    fn release_failure_overrides_success_or_primary_failure() {
        for primary in [Ok(()), Err("workload")] {
            let result =
                finalize_owned_terminal(primary, |primary| primary, || Err::<(), _>("release"));

            assert_eq!(result, Err(TerminalFailure::Release("release")));
        }
    }

    #[test]
    fn completion_failure_still_releases_once() {
        let releases = Cell::new(0);
        let result = finalize_owned_terminal(
            Ok::<(), &'static str>(()),
            |_| Err("watchdog"),
            || {
                releases.set(releases.get() + 1);
                Ok::<(), &'static str>(())
            },
        );

        assert_eq!(result, Err(TerminalFailure::Primary("watchdog")));
        assert_eq!(releases.get(), 1);
    }

    #[test]
    fn explicit_release_consumes_guard_without_drop_release() {
        let releases = Rc::new(Cell::new(0));
        let guard = FakeGuard {
            releases: Rc::clone(&releases),
            released: false,
        };

        let result = finalize_owned_terminal(
            Ok::<(), &'static str>(()),
            |primary| primary,
            || {
                guard.release();
                Ok::<(), &'static str>(())
            },
        );

        assert_eq!(result, Ok(()));
        assert_eq!(releases.get(), 1);
    }
}
