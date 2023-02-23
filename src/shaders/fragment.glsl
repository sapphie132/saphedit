#version 330 core
out vec4 FragColor;

in vec2 TexCoord;
uniform vec4 color;

// texture samplers
uniform sampler2D texture1;

void main()
{
	FragColor = texture(texture1, TexCoord) * color;
}