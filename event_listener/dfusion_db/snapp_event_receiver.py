import logging
from abc import ABC, abstractmethod
from typing import Dict, Any, Optional

from .database_interface import DatabaseInterface, MongoDbInterface


class SnappEventListener(ABC):
    """Abstract SnappEventReceiver class."""

    def __init__(self, database_interface: Optional[DatabaseInterface] = None):
        self.database = database_interface if database_interface else MongoDbInterface()
        self.logger = logging.getLogger(__name__)

    @abstractmethod
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        return  # pragma: no cover