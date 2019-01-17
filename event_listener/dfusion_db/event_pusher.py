from pymongo import MongoClient
from django.conf import settings

client = MongoClient(
    host=settings.MONGO_HOST,
    port=settings.MONGO_PORT
)
db = client.get_database(settings.DB_NAME)


def post_deposit(event: dict):
    """
    :param event: dict
    :return: bson.objectid.ObjectId
    """
    # Verify integrity of post data
    assert event.keys() == {'accountId', 'tokenId', 'amount', 'slot'}, "Unexpected Event Keys"
    assert all(isinstance(val, int) for val in event.values()), "One or more of deposit values not integer type"

    deposits = db.deposits
    deposit_id = deposits.insert_one(event).inserted_id
    return deposit_id


def post_transition(event: dict):
    """
    :param event: dict
    :return: bson.objectid.ObjectId
    """

    # Verify integrity of post data
    assert event.keys() == {'transitionType', 'to', 'from'}, "Unexpected Event Keys"
    _to = event['to']
    _from = event['from']
    _type = event['transitionType']

    assert isinstance(_to, str) and len(_to) == 64, "Transition to has unexpected values"
    assert isinstance(_from, str) and len(_from) == 64, "Transition from has unexpected values"
    assert isinstance(_type, int) and _type in {0, 1, 2}, "Transition type not recognized"

    transitions = db.transitions
    transition_id = transitions.insert_one(event).inserted_id
    return transition_id
