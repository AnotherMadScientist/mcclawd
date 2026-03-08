FROM python:3.11-slim
RUN pip install uv
WORKDIR /app
COPY requirements.txt .
RUN uv pip install --system -r requirements.txt
COPY entrypoint.py .
COPY config/ ./config/
RUN apt-get update && apt-get install -y --no-install-recommends curl && rm -rf /var/lib/apt/lists/*
EXPOSE 8083
CMD ["python", "entrypoint.py"]
