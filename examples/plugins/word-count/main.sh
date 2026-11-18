# 標準入力の JSON から selection を取り出して数える（jq があれば jq を使う）
input=$(cat)
if command -v jq >/dev/null 2>&1; then
  text=$(printf '%s' "$input" | jq -r '.note.selection // ""')
else
  text=$(printf '%s' "$input" | sed -n 's/.*"selection":"\([^"]*\)".*/\1/p')
fi
echo "$(printf '%s' "$text" | wc -w | tr -d ' ') 語です"
