"""Scheduler integration fixtures only: fake handles, no Game rows/processes."""
from pathlib import Path
import tempfile
import unittest

from balance_dispatch_control_d339_20260914 import DispatchControl, set_control
from balance_dispatch_queue_d339_20260914 import DispatchQueue


class FakeHandle:
    """Only natural poll completion is exposed; no kill/terminate operation."""
    def __init__(self):
        self.exit_code = None

    def poll(self):
        return self.exit_code


class DispatchIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='dispatch-queue-unit-')
        root = Path(self.temporary.name)
        self.path = root / 'control.json'
        self.control = DispatchControl(self.path, root / 'audit')
        self.started, self.handles, self.bindings, self.finished = [], {}, {}, []
        set_control(self.path, 'resume', 'scheduler unit fixture')

    def tearDown(self):
        self.temporary.cleanup()

    def launch(self, job, state):
        self.assertTrue(state.dispatchAllowed)
        self.assertNotIn(job, self.handles)
        handle = FakeHandle()
        self.started.append(job); self.handles[job] = handle
        self.bindings[job] = state.serialize()
        return handle

    def finish(self, job, handle, code):
        self.assertIs(handle, self.handles[job])
        self.assertIsNotNone(code)
        self.assertNotIn(job, self.finished)
        self.finished.append(job)
        return job

    def test_pause_after_sixteen_drains_and_waits_resume_keeps_cursor_without_duplicates(self):
        queue = DispatchQueue(range(20), self.launch, self.finish)
        queue.cycle(self.control)
        self.assertEqual(self.started, list(range(16)))
        self.assertEqual(queue.cursor, 16); self.assertEqual(len(queue.active), 16)
        first_binding = dict(self.bindings[0])
        set_control(self.path, 'pauseDispatch', 'unit pause')
        for job in range(8): self.handles[job].exit_code = 0
        queue.cycle(self.control)
        self.assertEqual(len(queue.completed), 8); self.assertEqual(len(queue.active), 8)
        self.assertEqual(len(self.started), 16)
        for job in range(8, 16): self.handles[job].exit_code = 0
        queue.cycle(self.control); queue.cycle(self.control)
        self.assertEqual(len(queue.active), 0)
        self.assertFalse(queue.finished); self.assertFalse(queue.complete)
        self.assertEqual(queue.remaining, (16, 17, 18, 19))
        set_control(self.path, 'resume', 'unit resume')
        queue.cycle(self.control)
        self.assertEqual(self.started, list(range(20)))
        self.assertEqual(len(set(self.started)), 20)
        self.assertEqual(self.bindings[0], first_binding)
        self.assertEqual(self.bindings[0]['revision'], 0)
        self.assertEqual(self.bindings[16]['revision'], 2)
        self.assertNotEqual(self.bindings[0]['rawSha256'], self.bindings[16]['rawSha256'])
        for job in range(16, 20): self.handles[job].exit_code = 0
        queue.cycle(self.control)
        self.assertTrue(queue.finished); self.assertTrue(queue.complete)
        self.assertEqual(self.finished, list(range(20)))

    def test_control_is_polled_before_each_launch_not_just_before_filling_sixteen_slots(self):
        def launch_and_pause(job, state):
            handle = self.launch(job, state)
            if job == 2: set_control(self.path, 'pauseDispatch', 'unit pause while filling slots')
            return handle
        queue = DispatchQueue(range(20), launch_and_pause, self.finish)
        queue.cycle(self.control)
        self.assertEqual(self.started, [0, 1, 2])
        self.assertEqual(queue.cursor, 3)
        self.assertFalse(queue.last_control.dispatchAllowed)

    def test_stop_after_active_drains_and_retains_unstarted_jobs_for_cancellation(self):
        queue = DispatchQueue(range(20), self.launch, self.finish)
        queue.cycle(self.control)
        set_control(self.path, 'stopAfterActive', 'unit terminal drain')
        queue.cycle(self.control)
        self.assertEqual(len(queue.active), 16)
        self.assertTrue(queue.stop_requested)
        set_control(self.path, 'resume', 'unit resume must not unlock drain')
        queue.cycle(self.control)
        self.assertEqual(len(self.started), 16)
        for handle in self.handles.values(): handle.exit_code = 0
        queue.cycle(self.control)
        self.assertTrue(queue.finished); self.assertFalse(queue.complete)
        self.assertEqual(queue.remaining, (16, 17, 18, 19))
        self.assertEqual(len(queue.completed), 16)
        self.assertEqual(queue.failures, [])

    def test_launch_error_is_attempted_once_and_other_handles_finish_naturally(self):
        attempts = []
        def launch_or_fail(job, state):
            attempts.append(job)
            if job == 3: raise OSError('unit launch failure')
            return self.launch(job, state)
        queue = DispatchQueue(range(10), launch_or_fail, self.finish)
        queue.cycle(self.control)
        self.assertEqual(attempts, [0, 1, 2, 3])
        self.assertEqual(queue.cursor, 4); self.assertEqual(len(queue.active), 3)
        self.assertEqual(queue.failures[0]['kind'], 'launch-error')
        set_control(self.path, 'resume', 'unit cannot undo launch failure')
        queue.cycle(self.control)
        self.assertEqual(attempts, [0, 1, 2, 3]); self.assertFalse(queue.finished)
        for handle in self.handles.values(): handle.exit_code = 0
        queue.cycle(self.control)
        self.assertTrue(queue.finished); self.assertFalse(queue.complete)
        self.assertEqual(queue.remaining, tuple(range(4, 10)))
        self.assertEqual(self.finished, [0, 1, 2])

    def test_invalid_control_pauses_new_jobs_but_still_reaps_natural_completions(self):
        queue = DispatchQueue(range(4), self.launch, self.finish, max_workers=2)
        queue.cycle(self.control)
        self.path.write_bytes(b'{')
        self.handles[0].exit_code = 0
        queue.cycle(self.control)
        self.assertEqual(self.finished, [0]); self.assertEqual(self.started, [0, 1])
        self.assertEqual(queue.cursor, 2); self.assertFalse(queue.finished)
        set_control(self.path, 'resume', 'unit repair', revision=4)
        queue.cycle(self.control)
        self.assertEqual(self.started, [0, 1, 2])

    def test_finish_validation_error_drains_without_retrying_other_jobs(self):
        def finish_or_fail(job, handle, code):
            if job == 0: raise ValueError('unit receipt callback failure')
            return self.finish(job, handle, code)
        queue = DispatchQueue(range(5), self.launch, finish_or_fail, max_workers=2)
        queue.cycle(self.control); self.handles[0].exit_code = 0
        queue.cycle(self.control)
        self.assertTrue(queue.stop_requested)
        self.assertEqual(queue.failures[0]['kind'], 'worker-or-receipt-error')
        self.assertEqual(self.started, [0, 1]); self.assertEqual(len(queue.active), 1)
        self.handles[1].exit_code = 0; queue.cycle(self.control)
        self.assertTrue(queue.finished); self.assertFalse(queue.complete)
        self.assertEqual(queue.remaining, (2, 3, 4))
        self.assertEqual(self.finished, [1])


if __name__ == '__main__':
    unittest.main(verbosity=2)
