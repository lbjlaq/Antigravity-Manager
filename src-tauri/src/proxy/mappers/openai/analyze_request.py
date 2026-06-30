import re
import sys

file_path = r'D:\32057\Files_of_Desktop\Academic\AI\antigravity-manage\09_intermediate_msg_fix_v3\Antigravity-Manager\src-tauri\src\proxy\mappers\openai\request.rs'

with open(file_path, 'r', encoding='utf-8') as f:
    content = f.read()

# Try to find the function pub fn transform_openai_request
match = re.search(r'pub fn transform_openai_request.*?\{', content, re.MULTILINE)
if not match:
    print('Function transform_openai_request not found')
    sys.exit(0)

print("Let's look at the implementation body of open_ai_to_gemini:")
body_start = match.end()
# Extract up to EOF or end of function heuristically
func_body = content[body_start:]
# print occurrences of generationConfig mapping
gen_config_lines = []
for line in func_body.split('\n'):
    if 'generationConfig' in line or 'temperature' in line or 'top_p' in line or 'max_tokens' in line or 'penalty' in line or 'response_format' in line or 'seed' in line:
        gen_config_lines.append(line.strip())

print("\n--- Generation Config related lines ---")
for l in gen_config_lines[:40]:  # Limit output
    print(l)

# Check for tools handling
tool_lines = []
for line in func_body.split('\n'):
    if 'toolConfig' in line or 'functionCallingConfig' in line or 'tool_choice' in line:
        tool_lines.append(line.strip())

print("\n--- Tool Config related lines ---")
for l in tool_lines[:40]:
    print(l)
