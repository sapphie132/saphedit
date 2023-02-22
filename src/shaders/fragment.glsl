#version 330 core
out vec4 FragColor;

in vec2 TexCoord;
uniform vec4 color;

// texture samplers
uniform sampler2D texture1;

void main()
{
	// linearly interpolate between both textures (80% container, 20% awesomeface)
	FragColor = vec4(1, 1, 1, texture(texture1, TexCoord).r);
}