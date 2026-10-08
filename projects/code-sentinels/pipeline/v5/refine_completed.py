"""Apply the corrected compositing algorithm to existing completed real videos only."""
from pathlib import Path
from extract_clip import extract
from merge_building import merge

HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
completed=sorted((HERE/'jobs').glob('*/video.json'))
for receipt in completed: extract(receipt.parent.name)
for work in sorted((PROJECT/'Content/Animations/v5/buildings').glob('*-work.json')):
    slug=work.stem[:-5]
    if all(work.with_name(f'{slug}-{s}.json').exists() for s in ['land','work','destroy']): merge(slug)
