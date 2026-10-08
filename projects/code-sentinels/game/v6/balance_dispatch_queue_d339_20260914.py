"""Process queue scheduling only; no Game state, costs, seeds or tick handling."""


class DispatchQueue:
    def __init__(self, jobs, launch, finish, max_workers=16):
        if not 1 <= max_workers <= 16:
            raise ValueError('Worker limit must remain within 1..16.')
        self.jobs = tuple(jobs)
        self.launch = launch
        self.finish = finish
        self.max_workers = max_workers
        self.cursor = 0
        self.active = []
        self.completed = []
        self.failures = []
        self.stop_requested = False
        self.last_control = None

    def request_drain(self, reason):
        self.stop_requested = True
        self.failures.append({'kind': 'internal-drain-request', 'reason': str(reason)})

    def cycle(self, control):
        """Reap naturally exited workers, then poll control before EVERY dispatch."""
        state = control.poll()
        self.last_control = state
        self.stop_requested = self.stop_requested or state.stopRequested
        outcomes = []
        for entry in list(self.active):
            code = entry['handle'].poll()
            if code is None:
                continue
            self.active.remove(entry)
            try:
                result = self.finish(entry['job'], entry['handle'], code)
                self.completed.append(result)
                outcomes.append(result)
            except Exception as exc:
                self.failures.append({'kind': 'worker-or-receipt-error', 'job': entry['job'],
                                      'errorType': type(exc).__name__, 'error': str(exc)})
                self.stop_requested = True
        while len(self.active) < self.max_workers and self.cursor < len(self.jobs):
            state = control.poll()
            self.last_control = state
            self.stop_requested = self.stop_requested or state.stopRequested
            if self.stop_requested or self.failures or not state.dispatchAllowed:
                break
            job = self.jobs[self.cursor]
            self.cursor += 1  # A refused OS launch remains an attempted case, never retried.
            try:
                handle = self.launch(job, state)
                self.active.append({'job': job, 'handle': handle})
            except Exception as exc:
                self.failures.append({'kind': 'launch-error', 'job': job,
                                      'errorType': type(exc).__name__, 'error': str(exc),
                                      'winerror': getattr(exc, 'winerror', None)})
                self.stop_requested = True
                break
        return outcomes

    @property
    def remaining(self):
        return self.jobs[self.cursor:]

    @property
    def finished(self):
        return not self.active and (self.cursor == len(self.jobs) or self.stop_requested or bool(self.failures))

    @property
    def complete(self):
        return self.finished and not self.failures and len(self.completed) == len(self.jobs)
