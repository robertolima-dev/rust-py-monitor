"""
Minimal Django settings for rust-py-monitor example.
"""
SECRET_KEY = "django-example-key-not-for-production"
DEBUG = True
ALLOWED_HOSTS = ["*"]

INSTALLED_APPS = [
    "django.contrib.contenttypes",
    "django.contrib.auth",
]

# rust-py-monitor: add MonitorMiddleware first so it wraps all requests
MIDDLEWARE = [
    "rust_py_monitor.django.MonitorMiddleware",
    "django.middleware.common.CommonMiddleware",
]

ROOT_URLCONF = "examples.django_app.urls"

DATABASES = {
    "default": {
        "ENGINE": "django.db.backends.sqlite3",
        "NAME": ":memory:",
    }
}

USE_TZ = True
DEFAULT_AUTO_FIELD = "django.db.models.BigAutoField"
