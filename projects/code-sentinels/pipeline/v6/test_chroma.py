"""Behavior fixtures for real-video background extraction, not production artwork."""
import unittest
from media import *
class ChromaTests(unittest.TestCase):
 def test_shaded_green_is_background(self):
  im=Image.new('RGB',(64,64),(0,64,0));out=key_green(im)
  self.assertEqual(np.asarray(out)[:,:,3].max(),0)
 def test_enclosed_green_costume_preserved(self):
  im=Image.new('RGB',(64,64),(0,255,0));d=ImageDraw.Draw(im);d.rectangle((10,10,54,54),fill=(235,235,235));d.rectangle((20,20,44,44),fill=(40,180,40))
  out=key_green(im);self.assertEqual(out.getpixel((30,30)),(40,180,40,255));self.assertEqual(out.getpixel((0,0))[3],0)
 def test_black_hair_preserved(self):
  im=Image.new('RGB',(64,64),(0,255,0));ImageDraw.Draw(im).ellipse((10,5,54,60),fill=(8,8,12));out=key_green(im)
  self.assertEqual(out.getpixel((30,30)),(8,8,12,255))
 def test_compressed_key_hole_between_limbs_removed(self):
  im=Image.new('RGB',(64,64),(0,255,0));d=ImageDraw.Draw(im);d.rectangle((10,10,54,54),fill=(48,42,66));d.rectangle((20,20,44,44),fill=(12,228,14))
  out=key_green(im);self.assertLess(out.getpixel((30,30))[3],5);self.assertEqual(out.getpixel((15,15)),(48,42,66,255))
 def test_non_key_background_not_silently_erased(self):
  out=key_green(Image.new('RGB',(64,64),(255,0,0)));self.assertEqual(out.getpixel((0,0)),(255,0,0,255))
 def test_reviewed_dark_prop_connected_to_hand_survives(self):
  im=Image.new('RGB',(64,64),(0,255,0));d=ImageDraw.Draw(im);d.rectangle((30,25,40,40),fill=(220,180,160));d.line((10,30,30,30),fill=(12,65,5),width=2)
  out=preserve_reviewed_dark_props(im,key_green(im));self.assertEqual(out.getpixel((15,30)),(12,65,5,255));self.assertEqual(out.getpixel((0,0))[3],0)
 def test_opt_in_does_not_preserve_exterior_dark_green(self):
  im=Image.new('RGB',(64,64),(0,64,0));ImageDraw.Draw(im).rectangle((25,25,40,40),fill=(220,180,160));out=preserve_reviewed_dark_props(im,key_green(im));self.assertEqual(out.getpixel((0,0))[3],0);self.assertEqual(out.getpixel((30,30)),(220,180,160,255))
 def test_opt_in_does_not_preserve_remote_dark_specks(self):
  im=Image.new('RGB',(64,64),(0,255,0));d=ImageDraw.Draw(im);d.rectangle((25,25,40,40),fill=(220,180,160));d.rectangle((5,5,9,9),fill=(0,64,0));out=preserve_reviewed_dark_props(im,key_green(im));self.assertEqual(out.getpixel((7,7))[3],0)
if __name__=='__main__':unittest.main()
