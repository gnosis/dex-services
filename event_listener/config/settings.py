"""
Django settings for dfusion project.

Generated by 'django-admin startproject' using Django 2.1.5.

For more information on this file, see
https://docs.djangoproject.com/en/2.1/topics/settings/

For the full list of settings and their values, see
https://docs.djangoproject.com/en/2.1/ref/settings/
"""
import environ
import os
from .log_settings import *

from event_listener.dfusion_db.abis import abi_file_path, load_json_file

env = environ.Env()

ROOT_DIR = environ.Path(__file__) - 3  # (/dex-services/config/settings.py - 4 = /dex-services)
env.read_env(str(ROOT_DIR.path('.env')))
env.read_env(str(ROOT_DIR.path('.env_db')))

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Quick-start development settings - unsuitable for production
# See https://docs.djangoproject.com/en/2.1/howto/deployment/checklist/

# SECURITY WARNING: keep the secret key used in production secret!
SECRET_KEY = 'fi5!d(uro*z=d6=wyr_yyclwto^hp1rmrla=+dq(jj&r&7*5e&'

# SECURITY WARNING: don't run with debug turned on in production!
DEBUG = True

ALLOWED_HOSTS = []


# Application definition

INSTALLED_APPS = [
    # django apps
    'django.contrib.admin',
    'django.contrib.auth',
    'django.contrib.contenttypes',
    'django.contrib.sessions',
    'django.contrib.messages',
    'django.contrib.staticfiles',

    # third party


    # gnosis apps
    'django_eth_events',

    # local apps
    'dfusion_db',

]

MIDDLEWARE = [
    'django.middleware.security.SecurityMiddleware',
    'django.contrib.sessions.middleware.SessionMiddleware',
    'django.middleware.common.CommonMiddleware',
    'django.middleware.csrf.CsrfViewMiddleware',
    'django.contrib.auth.middleware.AuthenticationMiddleware',
    'django.contrib.messages.middleware.MessageMiddleware',
    'django.middleware.clickjacking.XFrameOptionsMiddleware',
]

ROOT_URLCONF = 'config.urls'

TEMPLATES = [
    {
        'BACKEND': 'django.template.backends.django.DjangoTemplates',
        'DIRS': [os.path.join(BASE_DIR, '../../templates')]
        ,
        'APP_DIRS': True,
        'OPTIONS': {
            'context_processors': [
                'django.template.context_processors.debug',
                'django.template.context_processors.request',
                'django.contrib.auth.context_processors.auth',
                'django.contrib.messages.context_processors.messages',
            ],
        },
    },
]

WSGI_APPLICATION = 'config.wsgi.application'


# Database
# https://docs.djangoproject.com/en/2.1/ref/settings/#databases

DATABASES = {
    'default': {
        'ENGINE': 'django.db.backends.sqlite3',
        'NAME': os.path.join(BASE_DIR, 'db.sqlite3'),
    }
}


DB_HOST = os.environ['DB_HOST']
DB_PORT = int(os.environ['DB_PORT'])
DB_NAME = os.environ['DB_NAME']

# Password validation
# https://docs.djangoproject.com/en/2.1/ref/settings/#auth-password-validators

AUTH_PASSWORD_VALIDATORS = [
    {
        'NAME': 'django.contrib.auth.password_validation.UserAttributeSimilarityValidator',
    },
    {
        'NAME': 'django.contrib.auth.password_validation.MinimumLengthValidator',
    },
    {
        'NAME': 'django.contrib.auth.password_validation.CommonPasswordValidator',
    },
    {
        'NAME': 'django.contrib.auth.password_validation.NumericPasswordValidator',
    },
]


# Internationalization
# https://docs.djangoproject.com/en/2.1/topics/i18n/

LANGUAGE_CODE = 'en-us'

TIME_ZONE = 'UTC'

USE_I18N = True

USE_L10N = True

USE_TZ = True


# Static files (CSS, JavaScript, Images)
# https://docs.djangoproject.com/en/2.1/howto/static-files/

STATIC_URL = '/static/'


# ------------------------------------------------------------------------------
# ETHEREUM
# ------------------------------------------------------------------------------
ETH_BACKUP_BLOCKS = env.int('ETH_BACKUP_BLOCKS ', default=100)
ETH_PROCESS_BLOCKS = env.int('ETH_PROCESS_BLOCKS', default=100)
ETH_FILTER_PROCESS_BLOCKS = env.int('ETH_FILTER_PROCESS_BLOCKS', default=100000)

ETHEREUM_NODE_URL = env('ETHEREUM_NODE_URL', default='http://ganache-cli:8545')
ETHEREUM_MAX_WORKERS = env.int('ETHEREUM_MAX_WORKERS', default=10)
ETHEREUM_MAX_BATCH_REQUESTS = env.int('ETHEREUM_MAX_BATCH_REQUESTS', default=500)

# -------------------------
# GNOSIS ETHEREUM CONTRACTS
# -------------------------
ETH_EVENTS = [
    # {
    #     'ADDRESSES': [os.environ['SNAPP_CONTRACT_ADDRESS']],
    #     'EVENT_ABI': load_json_file(abi_file_path('SnappBase.json')),
    #     'EVENT_DATA_RECEIVER': 'event_listener.dfusion_db.event_receivers.EventDispatcher',
    #     'NAME': 'snappBaseEvents',
    #     'PUBLISH': True,
    # },
    {
        'ADDRESSES': [os.environ['SNAPP_CONTRACT_ADDRESS']],
        'EVENT_ABI': load_json_file(abi_file_path('SnappBase.json')),
        'EVENT_DATA_RECEIVER': 'event_listener.dfusion_db.event_receivers.DepositReceiver',
        'NAME': 'snappBaseDeposits',
        'PUBLISH': True,
    },
    {
        'ADDRESSES': [os.environ['SNAPP_CONTRACT_ADDRESS']],
        'EVENT_ABI': load_json_file(abi_file_path('SnappBase.json')),
        'EVENT_DATA_RECEIVER': 'event_listener.dfusion_db.event_receivers.StateTransitionReceiver',
        'NAME': 'snappBaseTransitions',
        'PUBLISH': True,
    },
    {
        'ADDRESSES': [os.environ['SNAPP_CONTRACT_ADDRESS']],
        'EVENT_ABI': load_json_file(abi_file_path('SnappBase.json')),
        'EVENT_DATA_RECEIVER': 'event_listener.dfusion_db.event_receivers.SnappInitializationReceiver',
        'NAME': 'snappBaseInit',
        'PUBLISH': True,
    },
]
