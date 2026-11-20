FROM gridcraft-harbor-base:latest
WORKDIR /app
# The starting workbook is generated in code (no binary fixtures in the repo).
RUN gridcraft-cli run --sample sales --out /app/sales.xlsx --quiet
