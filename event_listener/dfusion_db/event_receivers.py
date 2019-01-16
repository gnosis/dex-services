from django_eth_events.chainevents import AbstractEventReceiver
from pymongo import MongoClient

import logging

logger = logging.getLogger(__name__)


def post_event(event_dict):
    # client = MongoClient()
    # client = MongoClient('localhost', 27017)
    client = MongoClient('mongodb://localhost:27017/')
    db = client.test_database
    posts = db.posts
    post_id = posts.insert_one(event_dict).inserted_id
    return post_id


# def find_and_remove_event(event_dict):
#     client = MongoClient('mongodb://localhost:27017/')
#     db = client.test_database
#     posts = db.posts
#
#     document = posts.find(filter=event_dict)
#
#     db.collections.remove(event_dict, True)
#
#     document = posts.find(filter=event_dict)


class EventReceiver(AbstractEventReceiver):

    def save(self, decoded_event, block_info=None):
        res = {param['name']: param['value'] for param in decoded_event['params']}
        res.update({'type': decoded_event['name']})

        # Convert byte strings to hex
        for k, v in res.items():
            if isinstance(v, bytes):
                res[k] = v.hex()

        post_event(res)
        print("Event received {}".format(res))
        logger.info("Event received {}".format(res))

        # find_and_remove_event(event_dict=res)

    def rollback(self, decoded_event, block_info=None):
        pass
        # res = {param['name']: param['value'] for param in decoded_event['params']}
        # res.update({'type': decoded_event['name']})

        # find_and_remove_event(res)
