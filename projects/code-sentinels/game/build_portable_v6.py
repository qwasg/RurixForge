"""Build isolated V6 candidates or evidence-gated final archives."""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent / "v6"))
from portable_release import main

if __name__ == "__main__":
    main()
