"""Offline safety tests: these never enqueue a GPU or cloud generation."""
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import comfyui_provider as provider

class LocalProductionGuards(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
  self.project=Path(self.tmp.name);self.here=self.project/'pipeline/v5';self.folder=self.here/'jobs/nuclear-power-destroy'
  self.folder.mkdir(parents=True)
  self.spec={'id':'nuclear-power-destroy','category':'building','prompt':'A complete destruction event.',
             'references':[{'type':'first_frame','path':'reference.png'}]}
  (self.folder/'request.json').write_text(json.dumps(self.spec))
  self.patches=[patch.object(provider,'HERE',self.here),patch.object(provider,'PROJECT',self.project)]
  for p in self.patches:p.start();self.addCleanup(p.stop)
 def test_completed_source_never_calls_provider(self):
  source=self.project/'SourceMedia/saved.mp4';source.parent.mkdir();source.write_bytes(b'preserved source')
  (self.folder/'video.json').write_text(json.dumps({'fileRef':'SourceMedia/saved.mp4'}))
  with patch.object(provider,'base_url',side_effect=AssertionError('must not call backend')):
   provider.produce(self.spec['id'])
  self.assertEqual(source.read_bytes(),b'preserved source')
 def test_unknown_local_attempt_is_query_only(self):
  dest=self.folder/'recoveries'/provider.VERSION;dest.mkdir(parents=True)
  (dest/'recovery.json').write_text('{}');(dest/'attempt.json').write_text(json.dumps({'promptId':'saved-id'}))
  with patch.object(provider,'base_url',return_value='http://127.0.0.1:8188'),patch.object(provider,'find_history',return_value=None),patch.object(provider,'queued',return_value=False),patch.object(provider.requests,'post') as post:
   with self.assertRaisesRegex(RuntimeError,'unknown outcome'):
    provider.produce(self.spec['id'])
   post.assert_not_called()
  self.assertTrue((dest/'attempt.json').exists())
 def test_legacy_success_without_download_cannot_be_recreated(self):
  (self.folder/'attempt.json').write_text('{}');(self.folder/'created.json').write_text(json.dumps({'output':{'task_id':'old-id'}}))
  (self.folder/'status.json').write_text(json.dumps({'output':{'task_status':'SUCCEEDED'}}))
  with self.assertRaisesRegex(RuntimeError,'not conclusively failed'):provider.recovery_folder(self.folder)
 def test_confirmed_failure_preserves_and_archives_receipts(self):
  for name,data in {'attempt.json':{'old':'attempt'},'created.json':{'output':{'task_id':'old-id'}},'status.json':{'output':{'task_status':'FAILED'}},'failure.json':{'error':'old failure'}}.items():
   (self.folder/name).write_text(json.dumps(data))
  (self.here/'recharge-reconciliation-20260908T115926Z.json').write_text(json.dumps({'tasks':[{'taskId':'old-id','http':200,'status':'FAILED'}]}))
  original={p.name:p.read_bytes() for p in self.folder.glob('*.json')}
  dest=provider.recovery_folder(self.folder)
  for name in ['attempt.json','created.json','status.json','failure.json']:
   self.assertEqual((self.folder/name).read_bytes(),original[name]);self.assertEqual((dest/'legacy-receipts'/name).read_bytes(),original[name])
 def test_real_sampling_graph_connects_reference_to_h3(self):
  dest=self.folder/'work';dest.mkdir()
  with patch.object(provider,'upload',return_value='project/verified.png'):
   graph=provider.build_graph('http://127.0.0.1:8188',self.spec['id'],self.spec,dest,'fixed-prompt-id')
  self.assertEqual(graph['6']['class_type'],'MiniMaxH3ImageToVideo')
  self.assertEqual(graph['6']['inputs']['first_frame'],['20',0])
  self.assertEqual(graph['20']['class_type'],'LoadImage')
  self.assertEqual(graph['11']['class_type'],'SamplerCustomAdvanced')
  self.assertEqual(graph['11']['inputs']['latent_image'],['6',1])
  self.assertEqual(graph['9']['inputs']['steps'],8)
  self.assertEqual(graph['6']['inputs']['length']%17,5)
  self.assertFalse(any(n['class_type'] in {'RepeatImageBatch','ImageBatch','RepeatLatentBatch'} for n in graph.values()))
  self.assertEqual(graph['15']['inputs']['filename_prefix'],'code-sentinels-v5/nuclear-power-destroy/fixed-prompt-id')
 def test_final_get_report_reconciles_missing_individual_status(self):
  (self.folder/'attempt.json').write_text('{}');(self.folder/'created.json').write_text(json.dumps({'output':{'task_id':'old-id','task_status':'PENDING'}}))
  (self.here/'recharge-reconciliation-20260908T115926Z.json').write_text(json.dumps({'tasks':[{'taskId':'old-id','http':200,'status':'FAILED'}]}))
  dest=provider.recovery_folder(self.folder)
  record=json.loads((dest/'legacy-reconciliation.json').read_text())
  self.assertEqual(record['task']['status'],'FAILED');self.assertFalse(record['individualStatusPresent'])
  self.assertFalse((self.folder/'status.json').exists())

if __name__=='__main__':unittest.main()
