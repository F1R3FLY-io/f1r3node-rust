#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupKey<C, R> {
    pub context: C,
    pub request: R,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupPhase {
    Pending,
    Presence,
    Admission,
    Ready,
    Authorized,
    CallbackSucceeded,
    Succeeded,
    Failed,
    Cancelled,
}

impl StartupPhase {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupRequest<C, R> {
    pub key: StartupKey<C, R>,
    pub phase: StartupPhase,
    pub callback_required: bool,
}

#[derive(Debug)]
pub struct StartupCompletion<C, R> {
    context: Option<C>,
    current: Option<StartupRequest<C, R>>,
    active: Option<StartupKey<C, R>>,
    stopped: bool,
}

impl<C, R> Default for StartupCompletion<C, R> {
    fn default() -> Self {
        Self {
            context: None,
            current: None,
            active: None,
            stopped: false,
        }
    }
}

impl<C: Clone + Eq, R: Clone + Eq> StartupCompletion<C, R> {
    pub fn context(&self) -> Option<&C> { self.context.as_ref() }

    pub fn current(&self) -> Option<&StartupRequest<C, R>> { self.current.as_ref() }

    pub fn active(&self) -> Option<&StartupKey<C, R>> { self.active.as_ref() }

    pub fn is_stopped(&self) -> bool { self.stopped }

    pub fn publish_context(&mut self, context: C) -> bool {
        if self.stopped {
            return false;
        }
        self.cancel_current();
        self.context = Some(context);
        true
    }

    pub fn revoke_context(&mut self, context: &C) -> bool {
        if self.context.as_ref() != Some(context) {
            return false;
        }
        self.context = None;
        self.cancel_current();
        true
    }

    pub fn install(&mut self, key: StartupKey<C, R>, callback_required: bool) -> bool {
        if self.stopped
            || self.context.as_ref() != Some(&key.context)
            || self
                .current
                .as_ref()
                .is_some_and(|request| request.key == key)
            || self.active.as_ref() == Some(&key)
        {
            return false;
        }
        self.current = Some(StartupRequest {
            key,
            phase: StartupPhase::Pending,
            callback_required,
        });
        true
    }

    pub fn activate(&mut self) -> Option<StartupKey<C, R>> {
        if self.active.is_some() {
            return None;
        }
        let request = self.current.as_ref()?;
        if request.phase != StartupPhase::Pending || !self.is_current(&request.key) {
            return None;
        }
        let key = request.key.clone();
        self.current.as_mut()?.phase = StartupPhase::Presence;
        self.active = Some(key.clone());
        Some(key)
    }

    pub fn presence_complete(&mut self, key: &StartupKey<C, R>) -> bool {
        self.advance_scan(key, StartupPhase::Presence, StartupPhase::Admission)
    }

    pub fn scan_complete(&mut self, key: &StartupKey<C, R>) -> bool {
        self.advance_scan(key, StartupPhase::Admission, StartupPhase::Ready)
    }

    pub fn retire_active(&mut self, key: &StartupKey<C, R>) -> bool {
        if self.active.as_ref() != Some(key) {
            return false;
        }
        self.active = None;
        true
    }

    pub fn authorize_callback(&mut self, key: &StartupKey<C, R>) -> bool {
        if !self.is_current(key) {
            return false;
        }
        let Some(request) = self.current.as_mut() else {
            return false;
        };
        if request.phase != StartupPhase::Ready || !request.callback_required {
            return false;
        }
        request.phase = StartupPhase::Authorized;
        true
    }

    pub fn commit_success(&mut self, key: &StartupKey<C, R>) -> bool {
        if !self.is_current(key) {
            return false;
        }
        let Some(request) = self.current.as_mut() else {
            return false;
        };
        let expected = if request.callback_required {
            StartupPhase::CallbackSucceeded
        } else {
            StartupPhase::Ready
        };
        if request.phase != expected {
            return false;
        }
        request.phase = StartupPhase::Succeeded;
        true
    }

    pub fn callback_succeeded(&mut self, key: &StartupKey<C, R>) -> bool {
        if !self.is_current(key) {
            return false;
        }
        let Some(request) = self.current.as_mut() else {
            return false;
        };
        if request.phase != StartupPhase::Authorized || !request.callback_required {
            return false;
        }
        request.phase = StartupPhase::CallbackSucceeded;
        true
    }

    pub fn fail(&mut self, key: &StartupKey<C, R>) -> bool {
        if !self.is_current(key) {
            return false;
        }
        self.close(key, StartupPhase::Failed)
    }

    pub fn cancel(&mut self, key: &StartupKey<C, R>) -> bool {
        self.close(key, StartupPhase::Cancelled)
    }

    pub fn stop(&mut self) -> bool {
        if self.stopped {
            return false;
        }
        self.stopped = true;
        self.context = None;
        self.cancel_current();
        true
    }

    fn is_current(&self, key: &StartupKey<C, R>) -> bool {
        !self.stopped
            && self.context.as_ref() == Some(&key.context)
            && self
                .current
                .as_ref()
                .is_some_and(|request| request.key == *key)
    }

    fn advance_scan(
        &mut self,
        key: &StartupKey<C, R>,
        expected: StartupPhase,
        next: StartupPhase,
    ) -> bool {
        if self.active.as_ref() != Some(key) || !self.is_current(key) {
            return false;
        }
        let Some(request) = self.current.as_mut() else {
            return false;
        };
        if request.phase != expected {
            return false;
        }
        request.phase = next;
        true
    }

    fn close(&mut self, key: &StartupKey<C, R>, terminal: StartupPhase) -> bool {
        let Some(request) = self.current.as_mut() else {
            return false;
        };
        if request.key != *key || request.phase.is_terminal() {
            return false;
        }
        request.phase = terminal;
        true
    }

    fn cancel_current(&mut self) {
        if let Some(request) = self.current.as_mut() {
            if !request.phase.is_terminal() {
                request.phase = StartupPhase::Cancelled;
            }
        }
    }
}
