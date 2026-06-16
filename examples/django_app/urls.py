"""
URL configuration for the Django example app.

Run:
    pip install "rust-py-monitor[django]" django
    DJANGO_SETTINGS_MODULE=examples.django_app.settings python -m django runserver

Then:
    curl http://localhost:8000/
    curl http://localhost:8000/process/
    curl http://localhost:8000/metrics/
"""
from django.urls import path
from rust_py_monitor.prometheus import django_metrics_view

from . import views

urlpatterns = [
    path("", views.index),
    path("users/", views.users),
    path("process/", views.process_info),
    path("stats/", views.request_stats),
    path("requests/", views.recent_requests),
    # Prometheus scrape endpoint
    path("metrics/", django_metrics_view),
]
