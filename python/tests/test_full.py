from pathlib import Path

import pandas as pd

from rs1090 import decode

root = Path(__file__)


def test_full() -> None:
    data = pd.read_csv(
        root.parent.parent.parent / "crates/rs1090/data/long_flight.csv",
        names=["timestamp", "rawmsg"],
    )

    decoded = decode(
        data.rawmsg.str[18:],
        data.timestamp,
        reference=(43.3, 1.35),
    )

    assert data.shape[0] == len(decoded)
