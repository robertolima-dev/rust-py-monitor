"""
Shared test configuration.

Django is configured here once for all test modules that need it.
pytest-django picks up the settings before collecting tests, which ensures
fixtures like _dj_autoclear_mailbox work correctly across all test files.
"""
import django
from django.conf import settings


def pytest_configure():
    """Called by pytest before test collection. Configure Django once."""
    if not settings.configured:
        settings.configure(
            DATABASES={"default": {"ENGINE": "django.db.backends.sqlite3", "NAME": ":memory:"}},
            INSTALLED_APPS=[],
            USE_TZ=True,
            SECRET_KEY="test-secret-key-rust-py-monitor",
            # Required for pytest-django's _dj_autoclear_mailbox fixture.
            EMAIL_BACKEND="django.core.mail.backends.locmem.EmailBackend",
        )
        django.setup()
