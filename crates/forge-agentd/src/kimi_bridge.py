"""Private stdio adapter for the pinned official Kimi CLI runtime.

OAuth credentials stay in Kimi's own store. Only UI metadata, model deltas,
tool calls and token counts leave this process. Forge executes all tools.
"""
from __future__ import annotations

import asyncio
import json
import math
import sys
from datetime import datetime, timezone


def emit(value):
    print(json.dumps(value, ensure_ascii=False, allow_nan=False), flush=True)


def finite_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def managed_model(config):
    from kimi_cli.auth import KIMI_CODE_PLATFORM_ID
    from kimi_cli.auth.platforms import managed_provider_key

    provider_id = managed_provider_key(KIMI_CODE_PLATFORM_ID)
    candidates = [(name, model) for name, model in config.models.items()
                  if model.provider == provider_id]
    if not candidates or provider_id not in config.providers:
        return None
    _, model = next((item for item in candidates if item[0] == config.default_model), candidates[0])
    return config.providers[provider_id], model


def usage_windows(payload):
    """Normalize documented current and legacy CLI quota shapes; unknown stays unknown."""
    windows = []
    labels = {'limit5h': '5 小时', 'limit7d': '每周', 'monthTotal': '月度总额', 'monthCode': '月度编程'}
    current = payload.get('usages', {})
    if isinstance(current, dict):
        for name, value in current.items():
            if isinstance(value, dict) and finite_number(value.get('usedRatio')):
                windows.append({'id': name, 'label': labels.get(name, name),
                                'usedPercent': max(0, min(100, value['usedRatio'] * 100)),
                                'resetsAt': value.get('resetAt')})
    legacy = []
    if isinstance(payload.get('usage'), dict):
        legacy.append(('weekly', '每周', payload['usage']))
    for i, entry in enumerate(payload.get('limits') or []):
        if isinstance(entry, dict):
            detail = entry.get('detail') or entry
            window = entry.get('window') or {}
            label = f"{window.get('duration', '')} {window.get('timeUnit', '')}".strip() or f'使用窗口 {i + 1}'
            legacy.append((f'window-{i}', label, detail))
    if not windows:
        for name, label, value in legacy:
            limit = value.get('limit')
            used = value.get('used')
            if used is None and finite_number(limit) and finite_number(value.get('remaining')):
                used = limit - value['remaining']
            if finite_number(limit) and limit > 0 and finite_number(used):
                windows.append({'id': name, 'label': label, 'usedPercent': max(0, min(100, 100 * used / limit)),
                                'resetsAt': value.get('resetAt') or value.get('reset_at') or value.get('resetTime')})
    return windows


async def status(config, request):
    from kimi_cli.auth.oauth import OAuthManager, load_tokens
    from kimi_cli.auth.platforms import get_platform_by_id
    from kimi_cli.auth import KIMI_CODE_PLATFORM_ID
    from kimi_cli.utils.aiohttp import new_client_session

    selected = managed_model(config)
    connected = bool(selected and selected[0].oauth and load_tokens(selected[0].oauth))
    result = {'configured': connected, 'authMode': 'oauth' if connected else None,
              'account': None, 'quota': {'state': 'unknown', 'windows': []}}
    if connected and request.get('usage'):
        provider, _ = selected
        oauth = OAuthManager(config)
        try:
            await asyncio.wait_for(oauth.ensure_fresh(), timeout=12)
            key = oauth.resolve_api_key(provider.api_key, provider.oauth)
            platform = get_platform_by_id(KIMI_CODE_PLATFORM_ID)
            async with new_client_session() as session:
                async with session.get(platform.base_url.rstrip('/') + '/usages',
                                       headers={'Authorization': 'Bearer ' + key}, timeout=12) as response:
                    if response.status == 401:
                        result.update(configured=False, authMode=None)
                        result['error'] = 'Kimi 授权已失效，请重新登录'
                    response.raise_for_status()
                    payload = await response.json()
            windows = usage_windows(payload.get('quota', payload))
            result['quota'] = {'state': 'available' if windows else 'unavailable', 'windows': windows,
                               'updatedAt': datetime.now(timezone.utc).isoformat(), 'source': 'Kimi 官方 CLI'}
        except Exception:
            result['quota'] = {'state': 'unavailable', 'windows': [], 'error': '官方额度暂时无法读取'}
    emit(result)


async def chat(config, request):
    from kimi_cli.auth.oauth import OAuthManager, load_tokens
    from kimi_cli.llm import create_llm
    from kosong.message import Message, TextPart, ThinkPart, ToolCall, ToolCallPart
    from kosong.tooling import Tool

    selected = managed_model(config)
    if selected is None or not selected[0].oauth or not load_tokens(selected[0].oauth):
        emit({'error': 'CHANNEL_LOGIN_REQUIRED: 请先完成 Kimi 官方授权'})
        return
    provider, model = selected
    oauth = OAuthManager(config)
    await oauth.ensure_fresh()
    llm = create_llm(provider, model, thinking=request.get('thinking', False), oauth=oauth)
    if llm is None:
        emit({'error': 'CHANNEL_NOT_CONFIGURED: Kimi 官方模型未配置'})
        return
    history = []
    system = []
    for raw in request['messages']:
        thinking = raw.get('reasoning_content')
        raw = {k: v for k, v in raw.items() if k in ('role', 'name', 'content', 'tool_calls', 'tool_call_id')}
        if raw['role'] == 'system' and not history:
            system.append(raw.get('content') or '')
        else:
            # Preserve reasoning across tool turns, using Kosong's native content representation.
            message = Message.model_validate(raw)
            if thinking and raw['role'] == 'assistant':
                message.content.insert(0, ThinkPart(think=thinking))
            history.append(message)
    tools = [Tool.model_validate(item['function']) for item in request.get('tools', [])]
    if request.get('effort'):
        llm.chat_provider = llm.chat_provider.with_thinking(request['effort'])
    stream = await llm.chat_provider.generate('\n\n'.join(system), tools, history)
    content, reasoning, calls = [], [], []
    async for part in stream:
        if isinstance(part, TextPart):
            content.append(part.text)
            emit({'delta': 'text', 'text': part.text})
        elif isinstance(part, ThinkPart):
            reasoning.append(part.think)
            emit({'delta': 'reasoning', 'text': part.think})
        elif isinstance(part, ToolCall):
            calls.append(part)
            emit({'delta': 'tool', 'index': len(calls) - 1, 'id': part.id,
                  'name': part.function.name, 'text': part.function.arguments or ''})
        elif isinstance(part, ToolCallPart) and calls:
            calls[-1].merge_in_place(part)
            emit({'delta': 'tool', 'index': len(calls) - 1, 'id': calls[-1].id,
                  'name': calls[-1].function.name, 'text': part.arguments_part or ''})
    message = {'role': 'assistant', 'content': ''.join(content)}
    if reasoning:
        message['reasoning_content'] = ''.join(reasoning)
    if calls:
        message['tool_calls'] = [item.model_dump(exclude_none=True) for item in calls]
    usage = stream.usage
    emit({'choices': [{'message': message}], 'usage': None if usage is None else {
        'prompt_tokens': usage.input, 'completion_tokens': usage.output, 'total_tokens': usage.total}})


async def main():
    from kimi_cli.config import load_config
    from kimi_cli.auth.oauth import login_kimi_code, logout_kimi_code

    mode = sys.argv[1]
    request = json.loads(sys.stdin.readline() or '{}')
    config = load_config()
    if mode == 'status':
        await status(config, request)
    elif mode == 'chat':
        await chat(config, request)
    elif mode in ('login', 'logout'):
        events = login_kimi_code(config, open_browser=False) if mode == 'login' else logout_kimi_code(config)
        verification_shown = False
        async for event in events:
            if event.type == 'verification_url':
                if verification_shown:
                    emit({'error': 'Kimi 授权链接已过期，请重新登录'})
                    await events.aclose()
                    return
                verification_shown = True
                data = event.data or {}
                emit({'state': 'pending', 'authUrl': data.get('verification_url'), 'userCode': data.get('user_code')})
            elif event.type == 'success':
                emit({'state': 'authenticated' if mode == 'login' else 'logged_out'})
            elif event.type == 'error':
                emit({'error': 'Kimi 官方授权失败，请重试'})
                return


if __name__ == '__main__':
    try:
        asyncio.run(main())
    except Exception as exc:
        # Exceptions from SDKs can include HTTP bodies or credentials. Never relay them.
        emit({'error': 'KIMI_RUNTIME_ERROR: ' + type(exc).__name__})
        sys.exit(1)
