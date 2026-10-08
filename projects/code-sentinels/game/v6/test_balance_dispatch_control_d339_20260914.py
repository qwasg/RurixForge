"""Control-protocol unit fixtures only; no native jobs or balance evidence."""
from dataclasses import FrozenInstanceError
from contextlib import redirect_stdout
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import balance_dispatch_control_d339_20260914 as control


class DispatchControlTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='dispatch-control-unit-')
        self.root = Path(self.temporary.name)
        self.path = self.root / 'control.json'
        self.audit = self.root / 'audit'
        self.ctl = control.DispatchControl(self.path, self.audit)

    def tearDown(self):
        self.temporary.cleanup()

    def put(self, revision, command='resume', **changes):
        value = dict(schemaVersion=1, revision=revision, command=command,
                     requestedAtUtc='2026-09-14T00:00:00Z', reason='Unit control fixture')
        value.update(changes)
        raw = json.dumps(value, ensure_ascii=False).encode('utf-8')
        self.path.write_bytes(raw)
        return raw

    def events(self):
        return [json.loads(p.read_bytes()) for p in sorted((self.audit / 'events').glob('*.event.json'))]

    def test_initial_missing_control_is_audited_once_and_pauses(self):
        first = self.ctl.poll(); second = self.ctl.poll()
        self.assertEqual(first.mode, 'pauseDispatch')
        self.assertFalse(first.dispatchAllowed)
        self.assertFalse(first.stopRequested)
        self.assertIsNone(first.rawSha256)
        self.assertIn('FileNotFoundError', first.validationError)
        self.assertTrue(first.changed); self.assertFalse(second.changed)
        self.assertEqual(len(self.events()), 1)
        self.assertIsNone(self.events()[0]['rawFile'])

    def test_resume_pause_resume_and_snapshot_binding_are_exact(self):
        raw = self.put(0); first = self.ctl.poll()
        self.assertTrue(first.dispatchAllowed)
        self.assertEqual(first.rawSha256, hashlib.sha256(raw).hexdigest())
        self.assertEqual(first.revision, 0)
        bound = first.serialize()
        self.put(1, 'pauseDispatch'); paused = self.ctl.poll()
        self.assertFalse(paused.dispatchAllowed); self.assertFalse(paused.stopRequested)
        self.assertEqual(paused.lastAcceptedRevision, 1)
        self.put(2); resumed = self.ctl.poll()
        self.assertTrue(resumed.dispatchAllowed)
        self.assertEqual(bound, first.serialize())  # No reread or mutation of invocation binding.
        self.assertEqual(bound['revision'], 0)
        self.assertEqual(bound['rawSha256'], hashlib.sha256(raw).hexdigest())
        self.assertEqual(hashlib.sha256(Path(bound['auditEventPath']).read_bytes()).hexdigest(), bound['auditEventSha256'])
        bound['revision'] = 999
        self.assertEqual(first.revision, 0)
        with self.assertRaises(FrozenInstanceError):
            first.mode = 'pauseDispatch'

    def test_same_bytes_do_not_duplicate_audit_but_a_b_a_does(self):
        a = self.put(3); self.ctl.poll(); self.ctl.poll()
        self.path.write_bytes(b'{'); self.ctl.poll()
        self.path.write_bytes(a); returned = self.ctl.poll()
        events = self.events()
        self.assertEqual(len(events), 3)
        self.assertEqual([e['rawSha256'] for e in events],
                         [hashlib.sha256(raw).hexdigest() for raw in [a, b'{', a]])
        self.assertFalse(returned.dispatchAllowed)
        self.assertIn('strictly exceed', returned.validationError)
        self.assertEqual(returned.lastAcceptedRevision, 3)
        self.assertEqual(len({e['rawFile'] for e in events}), 3)
        for event, expected in zip(events, [a, b'{', a]):
            self.assertEqual((self.audit / event['rawFile']).read_bytes(), expected)

    def test_format_or_reason_change_requires_a_higher_revision(self):
        raw = self.put(5); self.ctl.poll()
        self.path.write_bytes(raw + b'\n'); state = self.ctl.poll()
        self.assertFalse(state.controlValid); self.assertFalse(state.dispatchAllowed)
        self.put(4); self.assertFalse(self.ctl.poll().dispatchAllowed)
        self.put(6); self.assertTrue(self.ctl.poll().dispatchAllowed)

    def test_stop_latches_through_pause_resume_invalid_and_reconstruction(self):
        self.put(0); self.ctl.poll()
        self.put(1, 'stopAfterActive'); stopped = self.ctl.poll()
        self.assertTrue(stopped.stopRequested); self.assertFalse(stopped.dispatchAllowed)
        for revision, command in [(2, 'resume'), (3, 'pauseDispatch'), (4, 'resume')]:
            self.put(revision, command); state = self.ctl.poll()
            self.assertEqual(state.mode, 'stopAfterActive')
            self.assertTrue(state.stopRequested); self.assertFalse(state.dispatchAllowed)
        self.path.write_bytes(b'not json'); self.assertTrue(self.ctl.poll().stopRequested)
        self.put(5); self.ctl.poll()
        recovered = control.DispatchControl(self.path, self.audit).poll()
        self.assertEqual(recovered.lastAcceptedRevision, 5)
        self.assertTrue(recovered.stopRequested); self.assertFalse(recovered.dispatchAllowed)
        self.assertTrue(any(e['ignoredBecauseStopLatched'] for e in self.events()))

    def test_invalid_fields_fail_closed_without_consuming_revision(self):
        self.put(1); self.ctl.poll()
        bad = [dict(extra='unexpected'), dict(command='kill'), dict(schemaVersion=True),
               dict(schemaVersion=2), dict(revision=True), dict(revision=-1),
               dict(requestedAtUtc='2026-09-14T12:00:00'), dict(requestedAtUtc='2026-09-14T12:00:00+08:00'),
               dict(reason=42)]
        for fields in bad:
            with self.subTest(fields=fields):
                value = dict(schemaVersion=1, revision=50, command='resume',
                             requestedAtUtc='2026-09-14T00:00:00Z', reason='fixture')
                value.update(fields); self.path.write_text(json.dumps(value), encoding='utf-8')
                state = self.ctl.poll()
                self.assertFalse(state.dispatchAllowed); self.assertFalse(state.controlValid)
                self.assertEqual(state.lastAcceptedRevision, 1)
        self.put(2); self.assertTrue(self.ctl.poll().dispatchAllowed)

    def test_malformed_utf8_duplicate_keys_and_nonobject_are_audited_exactly(self):
        self.put(0); self.ctl.poll()
        invalid = [b'\xff\xfe', b'{"revision":1,"revision":2}', b'[]', b'null', b'{']
        for raw in invalid:
            self.path.write_bytes(raw); state = self.ctl.poll()
            self.assertFalse(state.dispatchAllowed)
            event = self.events()[-1]
            self.assertFalse(event['valid'])
            self.assertEqual((self.audit / event['rawFile']).read_bytes(), raw)
            self.assertEqual(event['rawSha256'], hashlib.sha256(raw).hexdigest())

    def test_audit_distinguishes_json_parse_schema_and_revision_validity(self):
        self.path.write_bytes(b'{'); self.ctl.poll()
        self.assertFalse(self.events()[-1]['jsonParsed'])
        self.put(0, extra='unknown'); self.ctl.poll()
        event = self.events()[-1]
        self.assertTrue(event['jsonParsed']); self.assertFalse(event['schemaValid'])
        self.put(0); self.ctl.poll()
        self.path.write_bytes(self.path.read_bytes() + b'\n'); self.ctl.poll()
        event = self.events()[-1]
        self.assertTrue(event['jsonParsed']); self.assertTrue(event['schemaValid']); self.assertFalse(event['valid'])

    def test_unicode_escape_in_reason_cannot_break_raw_auditing(self):
        document = dict(schemaVersion=1, revision=0, command='resume',
                        requestedAtUtc='2026-09-14T00:00:00Z', reason='\ud800')
        self.path.write_bytes(json.dumps(document, ensure_ascii=True).encode('ascii'))
        state = self.ctl.poll()
        self.assertFalse(state.dispatchAllowed)
        self.assertIn('valid Unicode text', state.validationError)
        self.assertEqual(len(self.events()), 1)

    def test_read_error_pauses_without_changing_last_accepted_revision(self):
        self.put(2); self.ctl.poll()
        original = Path.read_bytes
        def fail_control(path):
            if path == self.path:
                raise PermissionError(13, 'fixture unavailable')
            return original(path)
        with patch.object(Path, 'read_bytes', fail_control):
            state = self.ctl.poll()
        self.assertFalse(state.dispatchAllowed)
        self.assertEqual(state.lastAcceptedRevision, 2)
        self.assertIn('PermissionError', state.validationError)
        self.assertIsNone(state.rawSha256)

    def test_audit_failure_never_authorizes_and_retries_same_change(self):
        self.put(0); self.ctl.poll(); self.put(1)
        with patch.object(self.ctl, '_record', side_effect=OSError('disk fixture full')):
            failed = self.ctl.poll()
        self.assertFalse(failed.dispatchAllowed); self.assertEqual(failed.lastAcceptedRevision, 0)
        self.assertIn('audit unavailable', failed.validationError)
        retried = self.ctl.poll()
        self.assertTrue(retried.dispatchAllowed); self.assertEqual(retried.lastAcceptedRevision, 1)
        self.assertEqual(len(self.events()), 2)

    def test_valid_stop_still_latches_in_memory_if_audit_write_fails(self):
        self.put(0); self.ctl.poll(); self.put(1, 'stopAfterActive')
        with patch.object(self.ctl, '_record', side_effect=OSError('storage fixture error')):
            stopped = self.ctl.poll()
        self.assertTrue(stopped.stopRequested); self.assertFalse(stopped.dispatchAllowed)
        self.put(2, 'resume'); state = self.ctl.poll()
        self.assertTrue(state.stopRequested); self.assertFalse(state.dispatchAllowed)

    def test_status_is_atomic_mutable_and_cannot_overwrite_audit_or_control(self):
        self.put(0); self.ctl.poll(); events_before = [(p, p.read_bytes()) for p in self.audit.rglob('*') if p.is_file()]
        status = self.root / 'status.json'
        self.ctl.write_status(status, details={'activePids': [], 'pausedAndDrained': False})
        self.ctl.write_status(status, details={'activePids': [], 'pausedAndDrained': True})
        self.assertTrue(json.loads(status.read_bytes())['details']['pausedAndDrained'])
        self.assertTrue(json.loads(status.read_bytes())['control']['dispatchAllowed'])
        for path, data in events_before:
            self.assertEqual(path.read_bytes(), data)
        for forbidden in [self.path, self.audit / 'events/status.json', self.audit / 'raw/status.json']:
            with self.assertRaises(ValueError): self.ctl.write_status(forbidden)
        self.assertFalse(list(self.root.glob('*.tmp')))

    def test_recovery_checks_captured_raw_bytes(self):
        self.put(0); self.ctl.poll()
        event = self.events()[0]; (self.audit / event['rawFile']).write_bytes(b'corrupt fixture')
        with self.assertRaises(control.AuditHistoryError):
            control.DispatchControl(self.path, self.audit)

    def test_setter_increments_revision_and_replaces_only_after_complete_json(self):
        first = control.set_control(self.path, 'resume', 'initial fixture')
        self.assertEqual(first['revision'], 0)
        old_bytes = self.path.read_bytes(); real_replace = control.os.replace
        def inspect_replace(source, destination):
            self.assertEqual(self.path.read_bytes(), old_bytes)
            self.assertEqual(control.parse_control(Path(source).read_bytes())['revision'], 1)
            return real_replace(source, destination)
        with patch.object(control.os, 'replace', inspect_replace):
            second = control.set_control(self.path, 'pauseDispatch', 'next fixture')
        self.assertEqual(second['revision'], 1)
        self.assertEqual(control.parse_control(self.path.read_bytes()), second)
        self.assertFalse(self.path.with_name(self.path.name + '.writer.lock').exists())
        with self.assertRaises(control.ControlValidationError):
            control.set_control(self.path, 'resume', 'stale fixture', revision=1)

    def test_setter_requires_explicit_repair_revision_and_respects_other_writer_lock(self):
        self.path.write_text('{', encoding='utf-8')
        with self.assertRaises(control.ControlValidationError):
            control.set_control(self.path, 'resume', 'repair fixture')
        document = control.set_control(self.path, 'pauseDispatch', 'repair fixture', revision=20)
        self.assertEqual(document['revision'], 20)
        lock = self.path.with_name(self.path.name + '.writer.lock'); lock.write_text('other writer', encoding='utf-8')
        before = self.path.read_bytes()
        with self.assertRaises(FileExistsError):
            control.set_control(self.path, 'resume', 'concurrent fixture')
        self.assertEqual(self.path.read_bytes(), before)
        self.assertEqual(lock.read_text(), 'other writer')

    def test_cli_setter_does_not_invoke_processes(self):
        output = io.StringIO()
        with redirect_stdout(output):
            result = control.main(['set', str(self.path), 'stopAfterActive', '--reason', 'unit fixture'])
        self.assertEqual(result, 0)
        self.assertEqual(json.loads(output.getvalue())['command'], 'stopAfterActive')
        self.assertTrue(self.ctl.poll().stopRequested)


if __name__ == '__main__':
    unittest.main(verbosity=2)
