"""Audited, fail-closed control for a single balance dispatch-loop owner.

Integration (no process management is performed by this module)::

    ctl = DispatchControl(control_path, audit_directory)
    state = ctl.poll()  # Immediately before each Popen, in the same main loop.
    if state.dispatchAllowed:
        invocation['dispatchControl'] = state.serialize()  # Do not reread file.
        # The caller may now launch its already-approved next job.
    # On pause, keep polling/waiting; on stop, drain active jobs then finish.
    ctl.write_status(status_path, details={'activePids': [], 'queueCursor': 0})

Modes are exactly resume, pauseDispatch, stopAfterActive. Stop is terminal even
across reconstruction from the same audit directory. Each observed byte change
(including A -> B -> A) has an exclusive raw copy and exclusive JSON event.
Unobserved transient file versions cannot be recorded. A dedicated audit_dir
must have only one DispatchControl writer; CLI setters use an exclusive lock.

CLI: python -B balance_dispatch_control_d339_20260914.py set CONTROL COMMAND
     --reason TEXT [--revision N]
Revision defaults to existing valid revision + 1, or 0 for a new file. Repairing
an invalid existing file requires an explicit revision above the controller's
lastAcceptedRevision (inspect status); the controller remains authoritative.
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
from datetime import datetime, timedelta, timezone
import hashlib
import json
import os
from pathlib import Path
import uuid

COMMANDS = frozenset({'resume', 'pauseDispatch', 'stopAfterActive'})
CONTROL_KEYS = frozenset({'schemaVersion', 'revision', 'command', 'requestedAtUtc', 'reason'})


class ControlValidationError(ValueError):
    pass


class AuditHistoryError(RuntimeError):
    pass


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ControlValidationError(f'Duplicate JSON key: {key!r}')
        result[key] = value
    return result


def validate_control(document: object) -> dict:
    if not isinstance(document, dict) or set(document) != CONTROL_KEYS:
        raise ControlValidationError('Control must have exactly schemaVersion, revision, command, requestedAtUtc, reason')
    if type(document['schemaVersion']) is not int or document['schemaVersion'] != 1:
        raise ControlValidationError('schemaVersion must be integer 1')
    if type(document['revision']) is not int or document['revision'] < 0:
        raise ControlValidationError('revision must be a nonnegative integer')
    if not isinstance(document['command'], str) or document['command'] not in COMMANDS:
        raise ControlValidationError('Unknown dispatch command')
    if not isinstance(document['reason'], str) or not isinstance(document['requestedAtUtc'], str):
        raise ControlValidationError('reason and requestedAtUtc must be strings')
    try:
        document['reason'].encode('utf-8')
    except UnicodeError as error:
        raise ControlValidationError('reason must contain valid Unicode text') from error
    try:
        timestamp = datetime.fromisoformat(document['requestedAtUtc'].replace('Z', '+00:00'))
    except ValueError as error:
        raise ControlValidationError('requestedAtUtc must be an ISO UTC timestamp') from error
    if timestamp.tzinfo is None or timestamp.utcoffset() != timedelta(0):
        raise ControlValidationError('requestedAtUtc must have a UTC offset')
    return dict(document)


def _decode_control(raw: bytes):
    try:
        return json.loads(raw.decode('utf-8-sig'), object_pairs_hook=_unique_object)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ControlValidationError(str(error)) from error


def parse_control(raw: bytes) -> dict:
    return validate_control(_decode_control(raw))


def _atomic_json(path: Path, document: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f'.{path.name}.{uuid.uuid4().hex}.tmp')
    data = (json.dumps(document, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False) + '\n').encode('utf-8')
    try:
        with temporary.open('xb') as stream:
            stream.write(data); stream.flush(); os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()  # Only this call's newly created temporary file.


def set_control(control_path: str | Path, command: str, reason: str, *, revision: int | None = None) -> dict:
    """Atomically set valid control; explicit revision is required for repair."""
    path = Path(control_path).resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    lock = path.with_name(path.name + '.writer.lock')
    # A concurrent/stale writer lock is never removed speculatively.
    with lock.open('x', encoding='utf-8') as stream:
        stream.write(str(os.getpid()))
    try:
        previous_revision = -1
        if path.exists():
            raw = path.read_bytes()
            try:
                previous_revision = parse_control(raw)['revision']
            except ControlValidationError:
                if revision is None:
                    raise ControlValidationError('Invalid existing control: provide an explicit revision above the controller watermark')
                try:
                    partial = json.loads(raw.decode('utf-8-sig'), object_pairs_hook=_unique_object)
                    if isinstance(partial, dict) and type(partial.get('revision')) is int:
                        previous_revision = max(-1, partial['revision'])
                except (UnicodeError, ValueError):
                    pass
        next_revision = previous_revision + 1 if revision is None else revision
        document = validate_control(dict(schemaVersion=1, revision=next_revision, command=command,
                                         requestedAtUtc=utc_now(), reason=reason))
        if next_revision <= previous_revision:
            raise ControlValidationError('New revision must exceed the existing readable revision')
        _atomic_json(path, document)
        return document
    finally:
        lock.unlink()  # This setter acquired this exact exclusive lock.


@dataclass(frozen=True)
class ControlState:
    mode: str
    dispatchAllowed: bool
    stopRequested: bool
    revision: int | None
    rawSha256: str | None
    lastAcceptedRevision: int | None
    requestedCommand: str | None
    requestedAtUtc: str | None
    reason: str | None
    controlValid: bool
    validationError: str | None
    polledAtUtc: str
    auditEventPath: str | None
    auditEventSha256: str | None
    changed: bool

    def serialize(self) -> dict:
        """A detached dictionary for an invocation; it performs no file read."""
        return asdict(self)


class DispatchControl:
    def __init__(self, control_path: str | Path, audit_dir: str | Path):
        self.control_path = Path(control_path).resolve()
        self.audit_dir = Path(audit_dir).resolve()
        self.raw_dir = self.audit_dir / 'raw'
        self.events_dir = self.audit_dir / 'events'
        self.raw_dir.mkdir(parents=True, exist_ok=True)
        self.events_dir.mkdir(parents=True, exist_ok=True)
        self._mode = 'pauseDispatch'
        self._stop = False
        self._accepted = None
        self._observation_key = None
        self._sequence = 0
        self._document = None
        self._raw_sha = None
        self._valid = False
        self._error = 'No control has been observed'
        self._event_path = None
        self._event_sha = None
        self._polled_at = utc_now()
        self._recover()

    @property
    def mode(self) -> str:
        return self._mode

    @property
    def dispatchAllowed(self) -> bool:
        return self._mode == 'resume' and not self._stop and self._valid

    @property
    def stopRequested(self) -> bool:
        return self._stop

    def _recover(self) -> None:
        previous_revision, stop = None, False
        for path in sorted(self.events_dir.glob('*.event.json')):
            try:
                raw = path.read_bytes(); event = json.loads(raw)
                if event['schemaVersion'] != 1 or event['controlPath'] != str(self.control_path):
                    raise ValueError('Audit belongs to another control path/schema')
                if event['sequence'] != self._sequence + 1 or event['acceptedRevisionBefore'] != previous_revision:
                    raise ValueError('Audit event sequence/revision chain is not intact')
                accepted = event['acceptedRevisionAfter']
                if accepted is not None and (type(accepted) is not int or accepted < 0
                                             or previous_revision is not None and accepted < previous_revision):
                    raise ValueError('Audit revision moved backwards')
                if stop and not event['stopRequestedAfter']:
                    raise ValueError('Audit attempted to clear terminal stop')
                if event['rawFile'] is not None:
                    captured = (self.audit_dir / event['rawFile']).resolve()
                    if not captured.is_relative_to(self.raw_dir) or _sha(captured.read_bytes()) != event['rawSha256']:
                        raise ValueError('Original observed control bytes/hash are missing or altered')
                if event['effectiveMode'] not in COMMANDS:
                    raise ValueError('Audit effective mode is invalid')
                if event['stopRequestedAfter'] and event['effectiveMode'] != 'stopAfterActive':
                    raise ValueError('Terminal stop has an inconsistent mode')
            except (OSError, ValueError, KeyError, TypeError) as error:
                raise AuditHistoryError(f'Cannot safely recover audit event {path}: {error}') from error
            self._sequence = event['sequence']; previous_revision = accepted
            stop = event['stopRequestedAfter']
            self._mode, self._stop, self._accepted = event['effectiveMode'], stop, accepted
            self._observation_key = event['observationKey']
            self._document = event['parsedControl']
            self._raw_sha, self._valid, self._error = event['rawSha256'], event['valid'], event['validationError']
            self._event_path, self._event_sha = str(path), _sha(raw)

    def _state(self, changed: bool) -> ControlState:
        document = self._document or {}
        return ControlState(mode=self.mode, dispatchAllowed=self.dispatchAllowed, stopRequested=self.stopRequested,
                            revision=document.get('revision'), rawSha256=self._raw_sha,
                            lastAcceptedRevision=self._accepted, requestedCommand=document.get('command'),
                            requestedAtUtc=document.get('requestedAtUtc'), reason=document.get('reason'),
                            controlValid=self._valid, validationError=self._error, polledAtUtc=self._polled_at,
                            auditEventPath=self._event_path, auditEventSha256=self._event_sha, changed=changed)

    def _record(self, event: dict, raw: bytes | None) -> tuple[str, str]:
        stem = f'{event["sequence"]:08d}-{uuid.uuid4().hex}'
        event['rawFile'] = None
        if raw is not None:
            capture = self.raw_dir / f'{stem}.raw'
            with capture.open('xb') as stream:
                stream.write(raw); stream.flush(); os.fsync(stream.fileno())
            event['rawFile'] = capture.relative_to(self.audit_dir).as_posix()
        path = self.events_dir / f'{stem}.event.json'
        data = (json.dumps(event, ensure_ascii=True, sort_keys=True, indent=2, allow_nan=False) + '\n').encode('utf-8')
        with path.open('xb') as stream:
            stream.write(data); stream.flush(); os.fsync(stream.fileno())
        return str(path), _sha(data)

    def poll(self) -> ControlState:
        """Read once, audit any observed change, then return the authorizing snapshot."""
        self._polled_at = utc_now()
        raw, read_error = None, None
        try:
            raw = self.control_path.read_bytes()
            raw_sha = _sha(raw); key = 'sha256:' + raw_sha
        except OSError as error:
            raw_sha = None
            read_error = f'{type(error).__name__}: {error}'
            key = f'unavailable:{type(error).__name__}:{error.errno}'
        if key == self._observation_key:
            return self._state(False)
        document, validation_error = None, read_error
        json_parsed, schema_valid, parsed_type = False, False, None
        if raw is not None:
            try:
                parsed = _decode_control(raw)
                json_parsed, parsed_type = True, type(parsed).__name__
                document = validate_control(parsed)
                schema_valid = True
                if self._accepted is not None and document['revision'] <= self._accepted:
                    raise ControlValidationError('revision must strictly exceed lastAcceptedRevision after any byte change')
            except ControlValidationError as error:
                validation_error = str(error)
        valid = validation_error is None
        accepted = document['revision'] if valid else self._accepted
        stop = self._stop or bool(valid and document['command'] == 'stopAfterActive')
        mode = 'stopAfterActive' if stop else document['command'] if valid else 'pauseDispatch'
        event = dict(schemaVersion=1, sequence=self._sequence + 1, observedAtUtc=self._polled_at,
                     controlPath=str(self.control_path), observationKey=key, rawSha256=raw_sha,
                     rawByteCount=len(raw) if raw is not None else None,
                     jsonParsed=json_parsed, schemaValid=schema_valid, parsedJsonType=parsed_type,
                     parsedControl=document, valid=valid, validationError=validation_error,
                     acceptedRevisionBefore=self._accepted, acceptedRevisionAfter=accepted,
                     previousMode=self._mode, effectiveMode=mode, stopRequestedAfter=stop,
                     ignoredBecauseStopLatched=bool(self._stop and valid and document['command'] != 'stopAfterActive'))
        try:
            event_path, event_sha = self._record(event, raw)
        except OSError as error:
            # Do not commit a new revision/key without its durable audit. Retry next poll.
            # A valid observed stop must still latch in memory if storage fails.
            self._stop = stop
            self._mode = 'stopAfterActive' if self._stop else 'pauseDispatch'
            self._valid = False; self._error = f'Control audit unavailable: {error}'
            self._raw_sha, self._document = raw_sha, document
            return self._state(True)
        self._sequence += 1; self._accepted, self._stop, self._mode = accepted, stop, mode
        self._observation_key, self._document, self._raw_sha = key, document, raw_sha
        self._valid, self._error = valid, validation_error
        self._event_path, self._event_sha = event_path, event_sha
        return self._state(True)

    def write_status(self, status_path: str | Path, *, details: dict | None = None) -> dict:
        """Atomically replace mutable status; event/raw history is never overwritten."""
        path = Path(status_path).resolve()
        if path == self.control_path or path.is_relative_to(self.raw_dir) or path.is_relative_to(self.events_dir):
            raise ValueError('Mutable status cannot overwrite control or immutable audit history')
        status = dict(schemaVersion=1, recordedAtUtc=utc_now(), control=self._state(False).serialize(),
                      details=dict(details or {}))
        _atomic_json(path, status)
        return status


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest='action', required=True)
    setter = subparsers.add_parser('set', help='Atomically write a new control revision')
    setter.add_argument('control_path', type=Path)
    setter.add_argument('command', choices=sorted(COMMANDS))
    setter.add_argument('--reason', required=True)
    setter.add_argument('--revision', type=int)
    args = parser.parse_args(argv)
    document = set_control(args.control_path, args.command, args.reason, revision=args.revision)
    print(json.dumps(document, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
