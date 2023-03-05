#version 330 core
in vec4 colour;
out vec4 FragColor;

void main()
{
	// linearly interpolate between both textures (80% container, 20% awesomeface)
	FragColor = colour;
}