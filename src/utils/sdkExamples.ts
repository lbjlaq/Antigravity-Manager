export type Protocol = 'openai' | 'anthropic' | 'gemini';

export const getPythonExample = (
    modelId: string,
    protocol: Protocol,
    port: number,
    apiKey: string,
    lang: string = 'en'
): string => {
    const isZh = lang.startsWith('zh');
    // 127.0.0.1 包示
    const baseUrlComment = isZh 
        ? '# \u63a8\u8350\u4f7f\u7528 127.0.0.1 \u4ee5\u907f\u514d\u90e8\u5206\u73af\u5883 IPv6 \u89e3\u6790\u5ef6\u8fdf\u95ee\u9898' 
        : '# Recommended: Use 127.0.0.1 to avoid IPv6 resolution delays';
    
    const baseUrl = `http://127.0.0.1:${port}/v1`;

    // 1. Anthropic Protocol
    if (protocol === 'anthropic') {
        const anthropicSdkNote = isZh
            ? '# \u6ce8\u610f: Antigravity \u652f\u6301\u4f7f\u7528 Anthropic SDK \u8c03\u7528\u4efb\u610f\u6a21\u578b'
            : '# Note: Antigravity supports calling any model via Anthropic SDK';

        return `from anthropic import Anthropic
 
 client = Anthropic(
     ${baseUrlComment}
     base_url="${`http://127.0.0.1:${port}`}",
     api_key="${apiKey}"
 )
 
 ${anthropicSdkNote}
 response = client.messages.create(
     model="${modelId}",
     max_tokens=1024,
     messages=[{"role": "user", "content": "Hello"}]
 )
 
 print(response.content[0].text)`;
    }

    // 2. Gemini Protocol (Native)
    if (protocol === 'gemini') {
        const rawBaseUrl = `http://127.0.0.1:${port}`;
        const installComment = isZh
            ? '# \u9700\u8981\u5b89\u88c5: pip install google-generativeai'
            : '# Install: pip install google-generativeai';
        const proxyComment = isZh
            ? '# \u4f7f\u7528 Antigravity \u4ee3\u7406\u5730\u5740 (\u63a8\u8350 127.0.0.1)'
            : '# Use Antigravity proxy address (Recommended 127.0.0.1)';

        return `${installComment}
import google.generativeai as genai

${proxyComment}
genai.configure(
    api_key="${apiKey}",
    transport='rest',
    client_options={'api_endpoint': '${rawBaseUrl}'}
)

model = genai.GenerativeModel('${modelId}')
response = model.generate_content("Hello")
print(response.text)`;
    }

    // 3. OpenAI Protocol
    if (modelId.startsWith('gemini-3-pro-image')) {
        const sizeComment = isZh
            ? '# \u65b9\u5f0f 1: \u4f7f\u7528 size \u53c2\u6570 (\u63a8\u8350)'
            : '# Method 1: Use size parameter (Recommended)';
        const supportComment = isZh
            ? '# \u652f\u6301: "1024x1024" (1:1), "1280x720" (16:9), "720x1280" (9:16), "1216x896" (4:3)'
            : '# Supports: "1024x1024" (1:1), "1280x720" (16:9), "720x1280" (9:16), "1216x896" (4:3)';
        const suffixComment = isZh
            ? '# \u65b9\u5f0f 2: \u4f7f\u7528\u6a21\u578b\u540e\u7f00'
            : '# Method 2: Use model suffix';
        const exampleComment = isZh
            ? '# \u4f8b\u5982: gemini-3-pro-image-16-9, gemini-3-pro-image-4-3'
            : '# e.g.: gemini-3-pro-image-16-9, gemini-3-pro-image-4-3';

        return `from openai import OpenAI
 
 client = OpenAI(
     base_url="${baseUrl}",
     api_key="${apiKey}"
 )
 
 response = client.chat.completions.create(
     model="${modelId}",
     ${sizeComment}
     ${supportComment}
     extra_body={ "size": "1024x1024" },
     
     ${suffixComment}
     ${exampleComment}
     # model="gemini-3-pro-image-16-9",
     messages=[{
         "role": "user",
         "content": "Draw a futuristic city"
     }]
 )
 
 print(response.choices[0].message.content)`;
    }

    return `from openai import OpenAI
 
 client = OpenAI(
     base_url="${baseUrl}",
     api_key="${apiKey}"
 )
 
 response = client.chat.completions.create(
     model="${modelId}",
     messages=[{"role": "user", "content": "Hello"}]
 )
 
 print(response.choices[0].message.content)`;
};

export const getNodeJSExample = (
    modelId: string,
    protocol: Protocol,
    port: number,
    apiKey: string,
    lang: string = 'en'
): string => {
    const baseUrl = `http://127.0.0.1:${port}/v1`;
    const isZh = lang.startsWith('zh');

    if (protocol === 'anthropic') {
        return `// npm install @anthropic-ai/sdk
import Anthropic from '@anthropic-ai/sdk';

const client = new Anthropic({
  baseURL: 'http://127.0.0.1:${port}',
  apiKey: '${apiKey}',
});

async function main() {
  const message = await client.messages.create({
    max_tokens: 1024,
    messages: [{ role: 'user', content: 'Hello' }],
    model: '${modelId}',
  });

  console.log(message.content[0].text);
}

main();`;
    }

    if (protocol === 'gemini') {
        const comment = isZh 
            ? '// \u63a8\u8350: \u4f7f\u7528 OpenAI SDK \u5305\u5bb9\u6a21\u5f0f\u8c03\u7528 Gemini'
            : '// Recommended: Use OpenAI SDK compatibility mode for Gemini';
        
        return `${comment}
// npm install openai
import OpenAI from 'openai';

const client = new OpenAI({
  apiKey: '${apiKey}',
  baseURL: 'http://127.0.0.1:${port}/v1',
});

async function main() {
  const response = await client.chat.completions.create({
    model: '${modelId}',
    messages: [{ role: 'user', content: 'Hello' }],
  });

  console.log(response.choices[0].message.content);
}

main();`;
    }

    if (modelId.startsWith('gemini-3-pro-image')) {
        const sizeComment = isZh 
            ? '// \u65b9\u5f0f 1: \u4f7f\u7528 size \u53c2\u6570 (\u63a8\u8350)' 
            : '// Method 1: Use size parameter (Recommended)';

        return `// npm install openai
import OpenAI from 'openai';

const client = new OpenAI({
  baseURL: '${baseUrl}',
  apiKey: '${apiKey}',
});

async function main() {
  const response = await client.chat.completions.create({
    model: '${modelId}',
    ${sizeComment}
    extra_body: { "size": "1024x1024" },
    messages: [{ role: 'user', content: 'Draw a futuristic city' }],
  });

  console.log(response.choices[0].message.content);
}

main();`;
    }

    return `// npm install openai
import OpenAI from 'openai';

const client = new OpenAI({
  baseURL: '${baseUrl}',
  apiKey: '${apiKey}',
});

async function main() {
  const chatCompletion = await client.chat.completions.create({
    messages: [{ role: 'user', content: 'Hello' }],
    model: '${modelId}',
  });

  console.log(chatCompletion.choices[0].message.content);
}

main();`;
};

export const getGoExample = (
    modelId: string,
    protocol: Protocol,
    port: number,
    apiKey: string,
    lang: string = 'en'
): string => {
    const baseUrl = `http://127.0.0.1:${port}/v1`;
    const isZh = lang.startsWith('zh');

    if (protocol === 'anthropic') {
        const note = isZh
            ? '// Anthropic \u5b98\u65b9\u76ee\u524d\u5c1a\u672a\u53d1\u5e03 Go SDK, \u5efa\u8bae\u4f7f\u7528\u6807\u51c6\u5e93\u6216\u7b2c\u4e09\u65b9\u5e93'
            : '// Anthropic does not have an official Go SDK yet, using standard library is recommended';

        return `package main
${note}
import (
	"bytes"
	"encoding/json"
	"fmt"
	"io/ioutil"
	"net/http"
)

func main() {
	url := "http://127.0.0.1:${port}/v1/messages"
	
	payload := map[string]interface{}{
		"model":      "${modelId}",
		"max_tokens": 1024,
		"messages": []map[string]string{
			{"role": "user", "content": "Hello"},
		},
	}
	jsonData, _ := json.Marshal(payload)

	req, _ := http.NewRequest("POST", url, bytes.NewBuffer(jsonData))
	req.Header.Set("x-api-key", "${apiKey}")
	req.Header.Set("anthropic-version", "2023-06-01")
	req.Header.Set("content-type", "application/json")

	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		fmt.Println("Error:", err)
		return
	}
	defer resp.Body.Close()
	
	body, _ := ioutil.ReadAll(resp.Body)
	fmt.Println(string(body))
}`;
    }

    if (protocol === 'gemini') {
        const comment = isZh 
            ? '// \u63a8\u8350: \u4f7f\u7528 OpenAI SDK \u5305\u5bb9\u6a21\u5f0f\u8c03\u7528 Gemini'
            : '// Recommended: Use OpenAI SDK compatibility mode for Gemini';

        return `package main
${comment}
// go get github.com/sashabaranov/go-openai
import (
	"context"
	"fmt"
	openai "github.com/sashabaranov/go-openai"
)

func main() {
	config := openai.DefaultConfig("${apiKey}")
	config.BaseURL = "http://127.0.0.1:${port}/v1"
	client := openai.NewClientWithConfig(config)

	resp, err := client.CreateChatCompletion(
		context.Background(),
		openai.ChatCompletionRequest{
			Model: "${modelId}",
			Messages: []openai.ChatCompletionMessage{
				{
					Role:    openai.ChatMessageRoleUser,
					Content: "Hello",
				},
			},
		},
	)

	if err != nil {
		fmt.Printf("Error: %v\n", err)
		return
	}
	fmt.Println(resp.Choices[0].Message.Content)
}`;
    }

    return `package main
// go get github.com/sashabaranov/go-openai
import (
	"context"
	"fmt"
	openai "github.com/sashabaranov/go-openai"
)

func main() {
	config := openai.DefaultConfig("${apiKey}")
	config.BaseURL = "http://127.0.0.1:${port}/v1"
	client := openai.NewClientWithConfig(config)

	resp, err := client.CreateChatCompletion(
		context.Background(),
		openai.ChatCompletionRequest{
			Model: "${modelId}",
			Messages: []openai.ChatCompletionMessage{
				{
					Role:    openai.ChatMessageRoleUser,
					Content: "Hello",
				},
			},
		},
	)

	if err != nil {
		fmt.Printf("Error: %v\n", err)
		return
	}
	fmt.Println(resp.Choices[0].Message.Content)
}`;
};

export const getCurLExample = (
    modelId: string,
    protocol: Protocol,
    port: number,
    apiKey: string
): string => {
    if (protocol === 'anthropic') {
        return `curl http://127.0.0.1:${port}/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: ${apiKey}" \
  -H "anthropic-version: 2023-06-01" \
  -d '{ 
    "model": "${modelId}",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'`;
    }

    return `curl http://127.0.0.1:${port}/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${apiKey}" \
  -d '{ 
    "model": "${modelId}",
    "messages": [{"role": "user", "content": "Hello"}]
  }'`;
};

export const getRustExample = (
    modelId: string,
    protocol: Protocol,
    port: number,
    apiKey: string,
    lang: string = 'en'
): string => {
    const isZh = lang.startsWith('zh');
    const comment = isZh 
        ? '// \u63a8\u8350: \u4f7f\u7528 async-openai \u5e93 (\u5305\u5bb9\u6a21\u5f0f)' 
        : '// Recommended: Use async-openai crate (Compatibility Mode)';

    return `// Cargo.toml: async-openai = "0.26.0"
use async_openai::
    types::{CreateChatCompletionRequestArgs, ChatCompletionRequestMessage, 
            ChatCompletionRequestUserMessageArgs},
    Client, config::OpenAIConfig,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ${comment}
    let config = OpenAIConfig::new()
        .with_api_key("${apiKey}")
        .with_api_base("http://127.0.0.1:${port}/v1");
    let client = Client::with_config(config);

    let request = CreateChatCompletionRequestArgs::default()
        .model("${modelId}")
        .messages([
            ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessageArgs::default()
                    .content("Hello")
                    .build()? 
            ),
        ])
        .build()?;

    let response = client.chat().create(request).await?;
    
    if let Some(choice) = response.choices.first() {
        println!("{}", choice.message.content.as_ref().unwrap_or(&"\".to_string()));
    }

    Ok(())
}`;
};
