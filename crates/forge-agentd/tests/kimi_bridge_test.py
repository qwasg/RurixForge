"""Offline contract tests against Forge's pinned official Kimi SDK runtime."""
import asyncio
import importlib.util
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

SPEC = importlib.util.spec_from_file_location('forge_kimi_bridge', Path(__file__).parents[1] / 'src' / 'kimi_bridge.py')
bridge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bridge)


class QuotaTests(unittest.TestCase):
    def test_current_and_legacy_quota_formats_preserve_missing_values(self):
        self.assertEqual(bridge.usage_windows({}), [])
        self.assertEqual(bridge.usage_windows({'usages': {'limit5h': {}}}), [])
        self.assertEqual(bridge.usage_windows({'usages': {'limit5h': {'usedRatio': float('nan')}}}), [])
        current = bridge.usage_windows({'usages': {'limit5h': {'usedRatio': .25, 'resetAt': '2026-10-07T16:00:00Z'},
                                                  'limit7d': {'usedRatio': .8}, 'monthCode': {'usedRatio': None}}})
        self.assertEqual([w['usedPercent'] for w in current], [25, 80])
        legacy = bridge.usage_windows({'usage': {'limit': 100, 'remaining': 60},
                                       'limits': [{'window': {'duration': 5, 'timeUnit': 'HOURS'},
                                                   'detail': {'limit': 20, 'used': 5, 'reset_at': '2026-10-07T16:00:00Z'}}]})
        self.assertEqual([w['usedPercent'] for w in legacy], [40, 25])
        self.assertEqual(legacy[1]['resetsAt'], '2026-10-07T16:00:00Z')


class ChatTests(unittest.IsolatedAsyncioTestCase):
    async def test_official_sdk_messages_tools_reasoning_and_usage_round_trip(self):
        from kosong.message import TextPart, ThinkPart, ToolCall, ToolCallPart
        from kosong.chat_provider import TokenUsage
        from kimi_cli.llm import LLM

        class Stream:
            usage = TokenUsage(input_other=90, input_cache_read=10, output=7)

            async def __aiter__(self):
                for part in [ThinkPart(think='分析'), TextPart(text='你好'),
                             ToolCall(id='tool-1', function=ToolCall.FunctionBody(name='scene_summary', arguments='{')),
                             ToolCallPart(arguments_part='"depth":1}')]:
                    yield part

        class Provider:
            def with_thinking(self, effort):
                self.effort = effort
                return self

            async def generate(self, system, tools, history):
                self.system, self.tools, self.history = system, tools, history
                return Stream()

        provider = Provider()
        llm = LLM(chat_provider=provider, max_context_size=262144, capabilities=set())
        selected = (SimpleNamespace(oauth=object(), api_key=object()), SimpleNamespace())
        request = {'thinking': True, 'effort': 'high', 'messages': [
            {'role': 'system', 'content': 'Forge tool policy'},
            {'role': 'user', 'content': '分析场景'},
            {'role': 'assistant', 'content': None, 'reasoning_content': '前一轮思考',
             'tool_calls': [{'type': 'function', 'id': 'old', 'function': {'name': 'scene_summary', 'arguments': '{}'}}]},
            {'role': 'tool', 'tool_call_id': 'old', 'content': 'scene evidence'},
        ], 'tools': [{'type': 'function', 'function': {'name': 'scene_summary', 'description': 'Read scene',
                                                      'parameters': {'type': 'object', 'properties': {}}}}]}
        with patch.object(bridge, 'managed_model', return_value=selected), patch.object(bridge, 'emit') as output, \
                patch('kimi_cli.auth.oauth.load_tokens', return_value=object()), \
                patch('kimi_cli.auth.oauth.OAuthManager', return_value=SimpleNamespace(ensure_fresh=AsyncMock())), \
                patch('kimi_cli.llm.create_llm', return_value=llm):
            await bridge.chat(SimpleNamespace(), request)
        self.assertEqual(provider.system, 'Forge tool policy')
        self.assertEqual(provider.tools[0].name, 'scene_summary')
        self.assertEqual(provider.history[1].content[0].think, '前一轮思考')
        self.assertEqual(provider.history[2].tool_call_id, 'old')
        self.assertEqual(provider.effort, 'high')
        records = [call.args[0] for call in output.call_args_list]
        self.assertEqual([record.get('delta') for record in records[:-1]], ['reasoning', 'text', 'tool', 'tool'])
        self.assertEqual(records[-1]['choices'][0]['message']['tool_calls'][0]['function']['arguments'], '{"depth":1}')
        self.assertEqual(records[-1]['usage'], {'prompt_tokens': 100, 'completion_tokens': 7, 'total_tokens': 107})

    async def test_missing_official_tokens_fails_without_contacting_the_model(self):
        with patch.object(bridge, 'managed_model', return_value=(SimpleNamespace(oauth=None), None)), \
                patch.object(bridge, 'emit') as output, patch('kimi_cli.llm.create_llm') as generate:
            await bridge.chat(SimpleNamespace(), {'messages': []})
        generate.assert_not_called()
        self.assertIn('CHANNEL_LOGIN_REQUIRED', output.call_args.args[0]['error'])


if __name__ == '__main__':
    unittest.main()
