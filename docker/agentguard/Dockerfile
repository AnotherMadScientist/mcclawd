FROM python:3.12-slim
RUN pip install uv
WORKDIR /app
COPY requirements.txt .
RUN uv pip install --system -r requirements.txt
COPY entrypoint.py .
RUN apt-get update && apt-get install -y --no-install-recommends curl && rm -rf /var/lib/apt/lists/*
EXPOSE 8084
CMD ["python", "entrypoint.py"]
