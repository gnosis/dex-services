#!/bin/bash

cd event_listener
rm -f db.sqlite3
python manage.py migrate
python manage.py run_listener
