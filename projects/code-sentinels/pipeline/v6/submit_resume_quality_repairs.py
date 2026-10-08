"""Submit only the six new quality-repair IDs prepared from concretely failed visual sources."""
from media import *
for row in read(HERE/'resume-quality-repairs.json')['repairs']:submit(HERE/'jobs'/row['new'])
status()
