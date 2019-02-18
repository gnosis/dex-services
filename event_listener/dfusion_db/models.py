from enum import Enum
from typing import NamedTuple, Dict, Any, List


class TransitionType(Enum):
    Deposit = 0
    Withdraw = 1
    Auction = 2


class StateTransition(NamedTuple):
    transition_type: TransitionType
    state_index: int
    state_hash: str
    slot: int

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "StateTransition":
        assert data.keys() == {'transitionType', 'stateIndex', 'stateHash', 'slot'}, \
            "Unexpected Event Keys: got {}".format(data.keys())
        _type = TransitionType(data['transitionType'])
        assert isinstance(data['stateIndex'],
                          int), "Transition to has unexpected values"
        _hash = data['stateHash']
        assert isinstance(_hash, str) and len(
            _hash) == 64, "Transition from has unexpected values"
        assert isinstance(data['slot'], int), "Transition slot not recognized"
        return StateTransition(_type, data['stateIndex'], _hash, data['slot'])


class Deposit(NamedTuple):
    account_id: int
    token_id: int
    amount: int
    slot: int
    slot_index: int

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "Deposit":
        assert all(k in data for k in(
            'accountId', 'tokenId', 'amount', 'slot', 'slotIndex')), \
            "Unexpected Event Keys"
        return Deposit(
            int(data['accountId']),
            int(data['tokenId']),
            int(data['amount']),
            int(data['slot']),
            int(data['slotIndex'])
        )


class Withdraw(NamedTuple):
    account_id: int
    token_id: int
    amount: int
    slot: int
    slot_index: int

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "Withdraw":
        assert all(k in data for k in(
            'accountId', 'tokenId', 'amount', 'slot', 'slotIndex')), \
            "Unexpected Event Keys"
        return Withdraw(
            int(data['accountId']),
            int(data['tokenId']),
            int(data['amount']),
            int(data['slot']),
            int(data['slotIndex'])
        )


class AccountRecord(NamedTuple):
    state_index: int
    state_hash: str
    balances: List[int]
