from __future__ import annotations

import pickle
from typing import (
    Any,
    Callable,
    Iterable,
    ParamSpec,
    Sequence,
    TypeVar,
    overload,
)

import pandas as pd

from ._rust import (
    aircraft_information,
    decode_1090,
    decode_1090_vec,
    decode_1090_with_reference,
    decode_1090t_vec,
    decode_bds05,
    decode_bds10,
    decode_bds17,
    decode_bds18,
    decode_bds19,
    decode_bds20,
    decode_bds21,
    decode_bds30,
    decode_bds40,
    decode_bds44,
    decode_bds45,
    decode_bds50,
    decode_bds60,
    decode_bds65,
    decode_flarm,
    decode_flarm_vec,
)
from .stubs import (
    Flarm,
    Message,
    is_bds05,
    is_bds06,
    is_bds08,
    is_bds09,
    is_bds10,
    is_bds17,
    is_bds20,
    is_bds30,
    is_bds40,
    is_bds44,
    is_bds50,
    is_bds60,
    is_bds61,
    is_bds62,
    is_bds65,
    is_df0,
    is_df4,
    is_df5,
    is_df11,
    is_df16,
    is_df17,
    is_df18,
    is_df20,
    is_df21,
)

try:
    # new in Python 3.12
    from itertools import batched  # type: ignore[attr-defined]
except ImportError:
    from itertools import islice

    T = TypeVar("T")

    def batched(iterable: Sequence[T], n: int) -> Iterable[tuple[T, ...]]:
        # batched('ABCDEFG', 3) --> ABC DEF G
        if n < 1:
            raise ValueError("n must be at least one")
        it = iter(iterable)
        while batch := tuple(islice(it, n)):
            yield batch


P = ParamSpec("P")


def unpickle_fun(fun: Callable[P, list[int]]) -> Callable[P, Any]:
    """Wrapper that unpickles the return value from Rust functions.

    The Rust bindings return pickled objects as list[int], which this
    wrapper converts back to the actual Python types.
    """

    def wrapped_fun(*args: P.args, **kwargs: P.kwargs) -> Any:
        int_list = fun(*args, **kwargs)
        return pickle.loads(bytes(int_list))

    return wrapped_fun


decode_bds05 = unpickle_fun(decode_bds05)  # type: ignore[arg-type]
decode_bds10 = unpickle_fun(decode_bds10)  # type: ignore[arg-type]
decode_bds17 = unpickle_fun(decode_bds17)  # type: ignore[arg-type]
decode_bds18 = unpickle_fun(decode_bds18)  # type: ignore[arg-type]
decode_bds19 = unpickle_fun(decode_bds19)  # type: ignore[arg-type]
decode_bds20 = unpickle_fun(decode_bds20)  # type: ignore[arg-type]
decode_bds21 = unpickle_fun(decode_bds21)  # type: ignore[arg-type]
decode_bds30 = unpickle_fun(decode_bds30)  # type: ignore[arg-type]
decode_bds40 = unpickle_fun(decode_bds40)  # type: ignore[arg-type]
decode_bds44 = unpickle_fun(decode_bds44)  # type: ignore[arg-type]
decode_bds45 = unpickle_fun(decode_bds45)  # type: ignore[arg-type]
decode_bds50 = unpickle_fun(decode_bds50)  # type: ignore[arg-type]
decode_bds60 = unpickle_fun(decode_bds60)  # type: ignore[arg-type]
decode_bds65 = unpickle_fun(decode_bds65)  # type: ignore[arg-type]


__all__ = [
    "Flarm",
    "Message",
    "batched",
    "decode",
    "decode_bds05",
    "decode_bds10",
    "decode_bds17",
    "decode_bds20",
    "decode_bds21",
    "decode_bds30",
    "decode_bds40",
    "decode_bds44",
    "decode_bds45",
    "decode_bds50",
    "decode_bds60",
    "flarm",
    "is_bds05",
    "is_bds06",
    "is_bds08",
    "is_bds09",
    "is_bds10",
    "is_bds17",
    "is_bds20",
    "is_bds30",
    "is_bds40",
    "is_bds44",
    "is_bds50",
    "is_bds60",
    "is_bds61",
    "is_bds62",
    "is_bds65",
    "is_df0",
    "is_df11",
    "is_df16",
    "is_df17",
    "is_df18",
    "is_df20",
    "is_df21",
    "is_df4",
    "is_df5",
    "aircraft_information",
]


@overload
def decode(
    msg: str,
    timestamp: None | float = None,
    *,
    reference: None | tuple[float, float] = None,
) -> Message: ...


@overload
def decode(
    msg: list[str] | pd.Series,
    timestamp: None | Sequence[float] | pd.Series = None,
    *,
    reference: None | tuple[float, float] = None,
    batch: int = 1000,
) -> list[Message]: ...


def decode(
    msg: str | list[str] | pd.Series,
    timestamp: None | float | Sequence[float] | pd.Series = None,
    *,
    reference: None | tuple[float, float] = None,
    batch: int = 1000,
) -> Message | list[Message]:
    if isinstance(msg, str):
        if reference is not None:
            payload = decode_1090_with_reference(msg, reference)
        else:
            payload = decode_1090(msg)

    else:
        if timestamp is not None and isinstance(timestamp, (int, float)):
            raise ValueError(
                "`timestamp` parameter must be a sequence of float"
            )
        if timestamp is not None and len(timestamp) != len(msg):
            raise ValueError("`msg` and `timestamp` must be of the same length")

        batches = list(batched(msg, batch))
        if timestamp is None:
            if reference is not None:
                raise ValueError(
                    "Provide timestamps in order to fully decode positions"
                )
            payload = decode_1090_vec(batches)
        else:
            ts = list(batched(timestamp, batch))
            payload = decode_1090t_vec(batches, ts, reference)

    return pickle.loads(bytes(payload))  # type: ignore[no-any-return]


@overload
def flarm(
    msg: str,
    timestamp: int,
    reference_latitude: float,
    reference_longitude: float,
    *,
    batch: int = 1000,
) -> Flarm: ...


@overload
def flarm(
    msg: Sequence[str],
    timestamp: Sequence[int],
    reference_latitude: Sequence[float],
    reference_longitude: Sequence[float],
    *,
    batch: int = 1000,
) -> list[Flarm]: ...


def flarm(
    msg: str | Sequence[str],
    timestamp: int | Sequence[int],
    reference_latitude: float | Sequence[float],
    reference_longitude: float | Sequence[float],
    *,
    batch: int = 1000,
) -> Flarm | list[Flarm]:
    if isinstance(msg, str):
        assert isinstance(timestamp, (int, float))
        assert isinstance(reference_latitude, (int, float))
        assert isinstance(reference_longitude, (int, float))
        payload = decode_flarm(
            msg,
            timestamp,
            reference_latitude,
            reference_longitude,
        )
    else:
        batches = list(batched(msg, batch))
        assert not isinstance(timestamp, (int, float))
        assert not isinstance(reference_latitude, (int, float))
        assert not isinstance(reference_longitude, (int, float))

        t = list(batched(timestamp, batch))
        reflat = list(batched(reference_latitude, batch))
        reflon = list(batched(reference_longitude, batch))

        payload = decode_flarm_vec(batches, t, reflat, reflon)

    return pickle.loads(bytes(payload))  # type: ignore[no-any-return]
