FROM python:3.11-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl build-essential && \
    rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
# Download the medium spacy model (~40MB) for better NER accuracy.
# The small model misses many PII entities; the large model (400MB)
# is overkill for our use case. Medium is the sweet spot.
RUN python -m spacy download en_core_web_md
# Pre-validate the model loads correctly at build time.
RUN python -c "import spacy; spacy.load('en_core_web_md'); print('spacy model ok')"

COPY entrypoint.py .
COPY config/ config/

RUN useradd -m -s /bin/bash security
USER security

EXPOSE 8082
CMD ["uvicorn", "entrypoint:app", "--host", "0.0.0.0", "--port", "8082"]
